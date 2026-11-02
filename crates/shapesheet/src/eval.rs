//! The evaluator (ADR 0053).
//!
//! [`eval`] computes one expression against an [`EvalContext`], which is
//! the seam for reading and writing cells (ADR 0054) and for recording
//! the structured warnings an unknown function or a missing reference
//! produces. Evaluation never fails the document: a problem is a
//! [`Value::Err`] error value or a warning, never a panic. `Err` is
//! reserved for a broken context.
//!
//! Task P6-05 provides the model-backed context
//! ([`crate::DocumentResolver`]); task P6-06 turns warnings into recalc
//! warnings and keeps the caller's cached value where the rules below
//! say so.
//!
//! # Evaluation rules
//!
//! - **Errors propagate** through operators and function arguments (the
//!   value's `#...` becomes the result). `IF` and `IFERROR` are the
//!   exceptions: `IF` evaluates only the taken branch, `IFERROR` returns
//!   its alternate expression when the primary one errors.
//! - **Coercion** is [`crate::coerce::coerce`], the one rule of
//!   `about-operators.md` (ADR 0052): an operand converts to the type an
//!   operator needs when it can, and becomes the
//!   typed default of that type (0, `""`, `FALSE`, black) with a
//!   [`EvalWarningKind::Coercion`] warning when it cannot. A comparison
//!   converts both sides to numbers, so `"10" > "9"` is `TRUE`.
//! - **Units** (ADR 0052): every numeric operation works in the family's
//!   canonical unit (inches, radians, fraction, seconds) and a computed
//!   number carries that canonical unit. A literal or a reference keeps
//!   the unit it carries. A plain number joins the family it is used
//!   with (`Width * 0.5`), and two operands that measure different
//!   things are `#VALUE!`. A quotient of two lengths is a plain ratio; a
//!   value type that cannot carry the result (`1 in * 1 in`,
//!   `5.2 cm ^ 1.5`) is `#DIM!`, the corner ADR 0052 keeps for
//!   dimensions. A non-finite result is `#NUM!`
//!   ([`EvalWarningKind::NonFinite`]), a division by zero `#DIV/0!`.
//! - **Unknown functions** (ADR 0053): a call the [`crate::FUNCTIONS`]
//!   table does not hold records one [`EvalWarningKind::UnknownFunction`]
//!   warning and the whole evaluation stands back. A reserved internal
//!   name (`_MOD`, `ELLIPSE_THETA`) gets the same treatment with its own
//!   warning text (ADR 0053), and the caller keeps the cell's cached
//!   value. The placeholder value [`ErrorCode::Na`] carries that result
//!   through [`Result`]; a caller must not write it when the warning was
//!   recorded. `IFERROR` does not catch it.
//! - **Argument counts** are checked against the table before any
//!   argument is evaluated: a wrong count is `#VALUE!` plus one
//!   [`EvalWarningKind::WrongArgumentCount`] warning, and no side effect
//!   (a `SETF` write) happens.
//! - **`GUARD(x)`** is a no-op value-wise and evaluates to `x` (the
//!   protection rule is P6-07's). **`SETF(target, value)`** writes
//!   `value` through [`EvalContext::set_cell`] and evaluates to `value`;
//!   a target the context refuses is `#REF!` with a
//!   [`EvalWarningKind::SetfRefused`] warning (ADR 0056). The context
//!   **records** the write; the recalc pass applies it through one write
//!   path, and an evaluation that stands back drops what it recorded.

use larrygram_model::{Color, SheetRef};

use crate::Error;
use crate::ast::{BinaryOp, Expr, ExprKind, Literal, NumberLiteral, UnaryOp};
use crate::coerce::{ValueKind, coerce};
use crate::formula::{CellRef, Formula};
use crate::functions::{FunctionKind, lookup_function};
use crate::units::{Unit, UnitFamily, parse_unit};
use crate::value::{ErrorCode, Number, Value};

/// The deepest expression tree [`eval`] walks (AGENTS.md section 6:
/// bounded recursion). The parser bounds its trees at
/// [`crate::MAX_NESTING`]; this bound also covers hand-built trees.
const MAX_EVAL_DEPTH: u32 = 64;

/// The longest string one `&` concatenation may build (AGENTS.md section
/// 6: capped allocations on input-derived values). Longer is `#VALUE!`.
const MAX_TEXT_LEN: usize = 1 << 20;

/// What a formula evaluation warned about (ADR 0053).
#[derive(Debug, Clone, PartialEq)]
pub struct EvalWarning {
    /// The sheet the formula being evaluated lives on.
    pub sheet: SheetRef,
    /// What happened.
    pub kind: EvalWarningKind,
}

/// The kind of an [`EvalWarning`] (ADR 0053).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum EvalWarningKind {
    /// A function the table does not implement (or a reserved
    /// `_`-prefixed internal name, ADR 0053); the cell keeps its cached
    /// value. This is the kind `recalc` maps to the keep-cached rule
    /// (ADR 0053), so the reserved case carries its own text here.
    UnknownFunction(String),
    /// A reference form the resolver cannot answer; the operand becomes
    /// `#REF!`.
    UnsupportedReference(String),
    /// A value had to be coerced to the type an operator needed and the
    /// conversion was lossy or defaulted (ADR 0052).
    Coercion(String),
    /// A computed number was not finite; it became `#NUM!` (P3-11).
    NonFinite,
    /// A call passed a count outside the table's bounds; the call
    /// evaluates to `#VALUE!` and nothing it contains is evaluated.
    WrongArgumentCount {
        /// The function name as written.
        function: String,
        /// The fewest arguments the function takes.
        min: u8,
        /// The most arguments it takes, or `None` for a variadic call.
        max: Option<u8>,
        /// How many arguments the call passed.
        got: usize,
    },
    /// A `SETF` whose target is not a writable cell reference; the call
    /// evaluates to `#REF!` (ADR 0056).
    SetfRefused(String),
    /// A dependency cycle was cut (ADR 0055).
    Cycle,
}

/// The cells an expression reads and writes while it evaluates (ADR 0054).
///
/// References are resolved with inheritance (ADR 0013): a formula a
/// master supplies evaluates against the sheet it is evaluated *for*.
pub trait EvalContext {
    /// The value of one referenced cell, resolved through the rungs of
    /// ADR 0013 and evaluated when its formula defines one.
    fn cell_value(&mut self, sheet: SheetRef, reference: &CellRef) -> Result<Value, Error>;

    /// Writes one referenced cell (basic `SETF`, ADR 0056).
    fn set_cell(&mut self, sheet: SheetRef, reference: &CellRef, value: Value)
    -> Result<(), Error>;

    /// Records one structured warning.
    fn warn(&mut self, warning: EvalWarning);

    /// Records that the evaluation stood back **silently** (a
    /// [`FunctionKind::VisioOwned`] call, task P7-31): the caller keeps
    /// the cell's cached value and reports nothing. The default is a
    /// no-op so existing contexts compile unchanged.
    fn keep_cached(&mut self) {}
}

/// Evaluates one expression on `sheet`.
///
/// Evaluation problems are [`Value::Err`] values; `Err` is reserved for
/// a broken context. When the evaluation records an
/// [`EvalWarningKind::UnknownFunction`] warning, the caller keeps the
/// cell's cached value (ADR 0053) and ignores the returned placeholder.
pub fn eval(expr: &Expr, sheet: SheetRef, ctx: &mut dyn EvalContext) -> Result<Value, Error> {
    let mut evaluator = Evaluator {
        sheet,
        ctx,
        unknown: None,
    };
    Ok(match evaluator.eval_expr(expr, 0)? {
        Flow::Value(value) => value,
        Flow::KeepCached => Value::Err(ErrorCode::Na),
    })
}

/// Parses and evaluates one formula source on `sheet`.
///
/// Same contract as [`eval`]; parse failures are [`Error::Syntax`].
pub fn eval_formula(
    formula: &Formula,
    sheet: SheetRef,
    ctx: &mut dyn EvalContext,
) -> Result<Value, Error> {
    let expr = crate::parse_formula(formula)?;
    eval(&expr, sheet, ctx)
}

/// What one subexpression produced.
enum Flow {
    /// A value (possibly an error value).
    Value(Value),
    /// An unknown function fired: evaluation stands back and the caller
    /// keeps its cached value (ADR 0053).
    KeepCached,
}

/// The kind of quantity a numeric operand measures (ADR 0052).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    /// A length: canonical inches.
    Length,
    /// An angle: canonical radians.
    Angle,
    /// A duration: canonical seconds.
    Duration,
}

impl Family {
    /// The unit a computed number of this family carries.
    const fn canonical_unit(self) -> Unit {
        match self {
            Family::Length => Unit::Inches,
            Family::Angle => Unit::Radians,
            Family::Duration => Unit::ElapsedSeconds,
        }
    }
}

/// A coerced numeric operand: the magnitude in the family's canonical
/// unit and the family it measures (`None` = a plain number).
#[derive(Debug, Clone, Copy)]
struct Num {
    /// The magnitude in the canonical unit of the family.
    magnitude: f64,
    /// What the operand measures.
    family: Option<Family>,
}

struct Evaluator<'c> {
    sheet: SheetRef,
    ctx: &'c mut dyn EvalContext,
    unknown: Option<String>,
}

impl Evaluator<'_> {
    fn warn(&mut self, kind: EvalWarningKind) {
        self.ctx.warn(EvalWarning {
            sheet: self.sheet,
            kind,
        });
    }

    fn eval_expr(&mut self, expr: &Expr, depth: u32) -> Result<Flow, Error> {
        if depth >= MAX_EVAL_DEPTH {
            // A hand-built tree deeper than the bound degrades instead of
            // overflowing the stack (AGENTS.md section 6).
            return Ok(Flow::Value(Value::Err(ErrorCode::Value)));
        }
        match &expr.kind {
            ExprKind::Literal(literal) => Ok(Flow::Value(self.literal(literal))),
            ExprKind::Ref(reference) => {
                Ok(Flow::Value(self.ctx.cell_value(self.sheet, reference)?))
            }
            ExprKind::Unary { op, operand } => self.eval_unary(*op, operand, depth),
            ExprKind::Binary { op, left, right } => self.eval_binary(*op, left, right, depth),
            ExprKind::Call { name, args } => self.eval_call(name, args, depth),
        }
    }

    fn literal(&mut self, literal: &Literal) -> Value {
        match literal {
            Literal::Number(number) => self.number_literal(number),
            Literal::Str(text) => Value::Str(text.clone()),
            Literal::Bool(flag) => Value::Bool(*flag),
        }
    }

    fn number_literal(&mut self, number: &NumberLiteral) -> Value {
        let Some(spelling) = number.unit.as_deref() else {
            return Value::Number(Number::new(number.value, Unit::Unitsless));
        };
        match parse_unit(spelling) {
            Ok(unit) => Value::Number(Number::new(number.value, unit)),
            Err(_) => {
                // The parser rejects unknown units (`SyntaxKind::
                // InvalidUnit`); only a hand-built tree can reach this.
                self.warn(EvalWarningKind::Coercion(format!(
                    "unknown unit of measure {spelling:?} in a number literal"
                )));
                Value::Err(ErrorCode::Value)
            }
        }
    }

    fn eval_unary(&mut self, op: UnaryOp, operand: &Expr, depth: u32) -> Result<Flow, Error> {
        let Flow::Value(value) = self.eval_expr(operand, depth + 1)? else {
            return Ok(Flow::KeepCached);
        };
        if let Some(blocked) = self.blocked(&value) {
            return Ok(Flow::Value(blocked));
        }
        let out = match op {
            // A no-op on the value (ADR 0051).
            UnaryOp::Plus => value,
            UnaryOp::Minus => {
                let num = self.coerce_num(&value);
                self.num_value(-num.magnitude, num.family)
            }
            UnaryOp::Percent => {
                let num = self.coerce_num(&value);
                self.num_value(num.magnitude / 100.0, num.family)
            }
        };
        Ok(Flow::Value(out))
    }

    fn eval_binary(
        &mut self,
        op: BinaryOp,
        left: &Expr,
        right: &Expr,
        depth: u32,
    ) -> Result<Flow, Error> {
        let Flow::Value(l) = self.eval_expr(left, depth + 1)? else {
            return Ok(Flow::KeepCached);
        };
        let Flow::Value(r) = self.eval_expr(right, depth + 1)? else {
            return Ok(Flow::KeepCached);
        };
        if let Some(blocked) = self.blocked(&l) {
            return Ok(Flow::Value(blocked));
        }
        if let Some(blocked) = self.blocked(&r) {
            return Ok(Flow::Value(blocked));
        }
        let out = match op {
            BinaryOp::Concat => self.concat(&l, &r),
            _ => {
                let ln = self.coerce_num(&l);
                let rn = self.coerce_num(&r);
                self.arith(op, &ln, &rn)
            }
        };
        Ok(Flow::Value(out))
    }

    fn concat(&mut self, left: &Value, right: &Value) -> Value {
        let text = text_of(left);
        let more = text_of(right);
        if text.len().saturating_add(more.len()) > MAX_TEXT_LEN {
            return Value::Err(ErrorCode::Value);
        }
        Value::Str(text + &more)
    }

    /// Operator arithmetic and comparison on two coerced operands
    /// (ADR 0052). Comparisons convert both sides to numbers and compare
    /// the canonical magnitudes.
    fn arith(&mut self, op: BinaryOp, left: &Num, right: &Num) -> Value {
        match op {
            BinaryOp::Eq => Value::Bool(left.magnitude == right.magnitude),
            BinaryOp::Ne => Value::Bool(left.magnitude != right.magnitude),
            BinaryOp::Lt => Value::Bool(left.magnitude < right.magnitude),
            BinaryOp::Le => Value::Bool(left.magnitude <= right.magnitude),
            BinaryOp::Gt => Value::Bool(left.magnitude > right.magnitude),
            BinaryOp::Ge => Value::Bool(left.magnitude >= right.magnitude),
            BinaryOp::Add | BinaryOp::Sub => {
                let Some(family) = Self::merged_family(left.family, right.family) else {
                    return Value::Err(ErrorCode::Value);
                };
                let magnitude = if op == BinaryOp::Add {
                    left.magnitude + right.magnitude
                } else {
                    left.magnitude - right.magnitude
                };
                self.num_value(magnitude, family)
            }
            BinaryOp::Mul => match (left.family, right.family) {
                (None, None) => self.num_value(left.magnitude * right.magnitude, None),
                (Some(family), None) | (None, Some(family)) => {
                    self.num_value(left.magnitude * right.magnitude, Some(family))
                }
                // Two dimensioned factors would need a dimension the
                // value type cannot carry (ADR 0052, `#DIM!`).
                (Some(_), Some(_)) => Value::Err(ErrorCode::Dim),
            },
            BinaryOp::Div => {
                if right.magnitude == 0.0 {
                    return Value::Err(ErrorCode::Div0);
                }
                match (left.family, right.family) {
                    (None, None) => self.num_value(left.magnitude / right.magnitude, None),
                    (Some(family), None) => {
                        self.num_value(left.magnitude / right.magnitude, Some(family))
                    }
                    // A quotient of two lengths is a plain ratio.
                    (Some(a), Some(b)) if a == b => {
                        self.num_value(left.magnitude / right.magnitude, None)
                    }
                    // Any other dimensioned quotient would need an
                    // inverse unit the value type cannot carry.
                    _ => Value::Err(ErrorCode::Dim),
                }
            }
            BinaryOp::Pow => {
                let exponent = right.magnitude;
                match left.family {
                    None => self.num_value(left.magnitude.powf(exponent), None),
                    Some(family) => {
                        // `5.2 cm ^ 1.5` is `#DIM!` (about-error-values).
                        if exponent.fract() != 0.0 {
                            return Value::Err(ErrorCode::Dim);
                        }
                        if exponent == 0.0 {
                            self.num_value(1.0, None)
                        } else if exponent == 1.0 {
                            self.num_value(left.magnitude, Some(family))
                        } else {
                            Value::Err(ErrorCode::Dim)
                        }
                    }
                }
            }
            // `&` is decided before arithmetic is called.
            BinaryOp::Concat => Value::Err(ErrorCode::Value),
        }
    }

    /// The family a binary result measures: `None` when the operands
    /// measure different things (the caller makes that `#VALUE!`).
    fn merged_family(left: Option<Family>, right: Option<Family>) -> Option<Option<Family>> {
        match (left, right) {
            (Some(a), Some(b)) => (a == b).then_some(Some(a)),
            (family, None) => Some(family),
            (None, family) => Some(family),
        }
    }

    fn eval_call(&mut self, name: &str, args: &[Expr], depth: u32) -> Result<Flow, Error> {
        let def = match lookup_function(name)? {
            Some(def) if def.kind == FunctionKind::VisioOwned => {
                // A Visio-owned call (`PAR`, `PNT`, `_WALKGLUE`,
                // `_XFTRIGGER`, ADR 0084 rule 2) stands back silently:
                // the cell keeps its cached value, nothing is reported
                // and its arguments are not evaluated.
                self.ctx.keep_cached();
                return Ok(Flow::KeepCached);
            }
            Some(def) => def,
            None => {
                self.stand_back(name, name.starts_with('_'));
                return Ok(Flow::KeepCached);
            }
        };
        let count = args.len();
        let within = count >= usize::from(def.min_args)
            && def.max_args.is_none_or(|max| count <= usize::from(max));
        if !within {
            self.warn(EvalWarningKind::WrongArgumentCount {
                function: String::from(def.name),
                min: def.min_args,
                max: def.max_args,
                got: count,
            });
            return Ok(Flow::Value(Value::Err(ErrorCode::Value)));
        }
        // Lazy and special forms take their arguments themselves.
        match def.name {
            "IF" => return self.eval_if(args, depth),
            "IFERROR" => return self.eval_iferror(args, depth),
            "SETF" => return self.eval_setf(args, depth),
            _ => {}
        }
        let mut values = Vec::with_capacity(count);
        for arg in args {
            match self.eval_expr(arg, depth + 1)? {
                Flow::Value(value) => values.push(value),
                Flow::KeepCached => return Ok(Flow::KeepCached),
            }
        }
        for value in &values {
            if let Some(blocked) = self.blocked(value) {
                return Ok(Flow::Value(blocked));
            }
        }
        Ok(Flow::Value(self.apply(def.name, &values)))
    }

    /// Records the one warning of an evaluation that stands back (ADR
    /// 0053): one per formula, naming the first offending function. A
    /// reserved internal name carries its own warning text, but stays an
    /// [`EvalWarningKind::UnknownFunction`] so the caller's keep-cached
    /// rule sees it.
    fn stand_back(&mut self, name: &str, reserved: bool) {
        if self.unknown.is_some() {
            return;
        }
        self.unknown = Some(String::from(name));
        let text = if reserved {
            format!("{name} (reserved internal name)")
        } else {
            String::from(name)
        };
        self.warn(EvalWarningKind::UnknownFunction(text));
    }

    fn eval_if(&mut self, args: &[Expr], depth: u32) -> Result<Flow, Error> {
        let Some((cond, branches)) = args.split_first() else {
            return Ok(Flow::Value(Value::Err(ErrorCode::Value)));
        };
        let Flow::Value(cond) = self.eval_expr(cond, depth + 1)? else {
            return Ok(Flow::KeepCached);
        };
        if let Some(blocked) = self.blocked(&cond) {
            return Ok(Flow::Value(blocked));
        }
        let taken = if self.coerce_bool(&cond) {
            branches.first()
        } else {
            branches.get(1)
        };
        let Some(branch) = taken else {
            return Ok(Flow::Value(Value::Err(ErrorCode::Value)));
        };
        // Only the taken branch evaluates (ADR 0060).
        self.eval_expr(branch, depth + 1)
    }

    fn eval_iferror(&mut self, args: &[Expr], depth: u32) -> Result<Flow, Error> {
        let Some((primary, rest)) = args.split_first() else {
            return Ok(Flow::Value(Value::Err(ErrorCode::Value)));
        };
        match self.eval_expr(primary, depth + 1)? {
            Flow::KeepCached => Ok(Flow::KeepCached),
            Flow::Value(Value::Err(_)) => {
                let Some(alternate) = rest.first() else {
                    return Ok(Flow::Value(Value::Err(ErrorCode::Value)));
                };
                self.eval_expr(alternate, depth + 1)
            }
            Flow::Value(value) => Ok(Flow::Value(value)),
        }
    }

    fn eval_setf(&mut self, args: &[Expr], depth: u32) -> Result<Flow, Error> {
        let (Some(target), Some(value_expr)) = (args.first(), args.get(1)) else {
            return Ok(Flow::Value(Value::Err(ErrorCode::Value)));
        };
        let Flow::Value(value) = self.eval_expr(value_expr, depth + 1)? else {
            return Ok(Flow::KeepCached);
        };
        // An error or non-finite value is not written into another cell.
        if let Some(blocked) = self.blocked(&value) {
            return Ok(Flow::Value(blocked));
        }
        if let ExprKind::Ref(reference) = &target.kind {
            return match self.ctx.set_cell(self.sheet, reference, value.clone()) {
                Ok(()) => Ok(Flow::Value(value)),
                Err(_) => {
                    self.warn(EvalWarningKind::SetfRefused(
                        reference.name().unwrap_or_default(),
                    ));
                    Ok(Flow::Value(Value::Err(ErrorCode::Ref)))
                }
            };
        }
        // Not a cell reference: nothing to write. Evaluate the target
        // expression anyway, so its own warnings still record.
        match self.eval_expr(target, depth + 1)? {
            Flow::KeepCached => Ok(Flow::KeepCached),
            Flow::Value(_) => {
                self.warn(EvalWarningKind::SetfRefused(String::from(
                    "not a cell reference",
                )));
                Ok(Flow::Value(Value::Err(ErrorCode::Ref)))
            }
        }
    }

    /// Applies one known function to its evaluated arguments.
    fn apply(&mut self, name: &str, args: &[Value]) -> Value {
        match name {
            "NOT" => {
                let flag = self.coerce_bool(&arg_at(args, 0));
                Value::Bool(!flag)
            }
            "AND" | "OR" => {
                // Every argument coerces, so the warnings do not depend
                // on which operand short-circuits.
                let flags: Vec<bool> = args.iter().map(|arg| self.coerce_bool(arg)).collect();
                Value::Bool(if name == "AND" {
                    flags.iter().all(|flag| *flag)
                } else {
                    flags.iter().any(|flag| *flag)
                })
            }
            "MIN" => self.extreme(args, true),
            "MAX" => self.extreme(args, false),
            "SUM" => self.sum(args),
            "ABS" => {
                let num = self.coerce_num(&arg_at(args, 0));
                self.num_value(num.magnitude.abs(), num.family)
            }
            "INT" => {
                let num = self.coerce_num(&arg_at(args, 0));
                self.num_value(num.magnitude.floor(), num.family)
            }
            "ROUND" | "TRUNC" => {
                let num = self.coerce_num(&arg_at(args, 0));
                let digits = self.num_at(args, 1).magnitude;
                self.rounded(&num, digits, name == "TRUNC")
            }
            "CEILING" | "FLOOR" => {
                let num = self.coerce_num(&arg_at(args, 0));
                let multiple = match args.get(1) {
                    Some(value) => self.coerce_num(value),
                    None => Num {
                        magnitude: 1.0,
                        family: None,
                    },
                };
                self.stepped(&num, &multiple, name == "CEILING")
            }
            "MODULUS" | "MOD" => {
                let num = self.coerce_num(&arg_at(args, 0));
                let divisor = self.num_at(args, 1);
                self.modulus(&num, &divisor)
            }
            "SIGN" => {
                let num = self.coerce_num(&arg_at(args, 0));
                let fuzz = match args.get(1) {
                    Some(value) => self.coerce_num(value).magnitude.abs(),
                    None => 0.0,
                };
                let sign = if num.magnitude.abs() <= fuzz {
                    0.0
                } else if num.magnitude < 0.0 {
                    -1.0
                } else {
                    1.0
                };
                self.num_value(sign, None)
            }
            "POW" => {
                let base = self.coerce_num(&arg_at(args, 0));
                let exponent = self.num_at(args, 1);
                self.arith(BinaryOp::Pow, &base, &exponent)
            }
            "SQRT" | "EXP" => {
                let num = self.coerce_num(&arg_at(args, 0));
                if num.family.is_some() {
                    // A fractional power of a dimensioned value is
                    // `#DIM!` (about-error-values.md).
                    return Value::Err(ErrorCode::Dim);
                }
                if name == "EXP" {
                    return self.num_value(num.magnitude.exp(), None);
                }
                if num.magnitude < 0.0 {
                    return Value::Err(ErrorCode::Num);
                }
                self.num_value(num.magnitude.sqrt(), None)
            }
            "LN" | "LOG10" => {
                let num = self.coerce_num(&arg_at(args, 0));
                if num.family.is_some() {
                    return Value::Err(ErrorCode::Dim);
                }
                if num.magnitude <= 0.0 {
                    return Value::Err(ErrorCode::Num);
                }
                let out = if name == "LN" {
                    num.magnitude.ln()
                } else {
                    num.magnitude.log10()
                };
                self.num_value(out, None)
            }
            "PI" => self.num_value(std::f64::consts::PI, None),
            "SIN" | "COS" | "TAN" | "SINH" | "COSH" | "TANH" => {
                let Some(angle) = self.angle_arg(&arg_at(args, 0)) else {
                    return Value::Err(ErrorCode::Value);
                };
                let out = match name {
                    "SIN" => angle.sin(),
                    "COS" => angle.cos(),
                    "TAN" => angle.tan(),
                    "SINH" => angle.sinh(),
                    "COSH" => angle.cosh(),
                    _ => angle.tanh(),
                };
                self.num_value(out, None)
            }
            "ASIN" | "ACOS" | "ATAN" => {
                let num = self.coerce_num(&arg_at(args, 0));
                if num.family.is_some() {
                    return Value::Err(ErrorCode::Value);
                }
                if name != "ATAN" && num.magnitude.abs() > 1.0 {
                    return Value::Err(ErrorCode::Num);
                }
                let out = match name {
                    "ASIN" => num.magnitude.asin(),
                    "ACOS" => num.magnitude.acos(),
                    _ => num.magnitude.atan(),
                };
                self.num_value(out, Some(Family::Angle))
            }
            "ATAN2" => {
                let y = self.coerce_num(&arg_at(args, 0));
                let x = self.num_at(args, 1);
                match Self::merged_family(y.family, x.family) {
                    None => Value::Err(ErrorCode::Value),
                    Some(_) => self.num_value(y.magnitude.atan2(x.magnitude), Some(Family::Angle)),
                }
            }
            "DEG" => {
                let Some(radians) = self.angle_arg(&arg_at(args, 0)) else {
                    return Value::Err(ErrorCode::Value);
                };
                self.number_result(radians.to_degrees(), Unit::Degrees)
            }
            "RAD" => {
                let num = self.coerce_num(&arg_at(args, 0));
                let degrees = match num.family {
                    None => num.magnitude,
                    Some(Family::Angle) => num.magnitude.to_degrees(),
                    Some(_) => return Value::Err(ErrorCode::Value),
                };
                self.number_result(degrees.to_radians(), Unit::Radians)
            }
            "STRSAME" => {
                let a = text_of(&arg_at(args, 0));
                let b = text_of(&arg_at(args, 1));
                let ignore_case = match args.get(2) {
                    Some(value) => self.coerce_bool(value),
                    None => false,
                };
                let same = if ignore_case {
                    a.eq_ignore_ascii_case(&b)
                } else {
                    a == b
                };
                Value::Bool(same)
            }
            "RGB" => self.rgb(args),
            "INDEX" => self.index(args),
            "LOOKUP" => self.lookup(args),
            "NA" => Value::Err(ErrorCode::Na),
            "GUARD" => arg_at(args, 0),
            // The table and this match never disagree; a disagreement
            // would mean a row landed in [`crate::FUNCTIONS`] without an
            // evaluator.
            _ => Value::Err(ErrorCode::Value),
        }
    }

    fn num_at(&mut self, args: &[Value], index: usize) -> Num {
        self.coerce_num(&arg_at(args, index))
    }

    fn extreme(&mut self, args: &[Value], minimum: bool) -> Value {
        let mut best: Option<Num> = None;
        for arg in args {
            let num = self.coerce_num(arg);
            let better = match best {
                None => true,
                Some(b) => {
                    if minimum {
                        num.magnitude < b.magnitude
                    } else {
                        num.magnitude > b.magnitude
                    }
                }
            };
            if better {
                best = Some(num);
            }
        }
        match best {
            Some(num) => self.num_value(num.magnitude, num.family),
            None => Value::Err(ErrorCode::Value),
        }
    }

    fn sum(&mut self, args: &[Value]) -> Value {
        let mut total = Num {
            magnitude: 0.0,
            family: None,
        };
        for arg in args {
            let num = self.coerce_num(arg);
            let Some(family) = Self::merged_family(total.family, num.family) else {
                return Value::Err(ErrorCode::Value);
            };
            total = Num {
                magnitude: total.magnitude + num.magnitude,
                family,
            };
        }
        self.num_value(total.magnitude, total.family)
    }

    fn rounded(&mut self, num: &Num, digits: f64, truncate: bool) -> Value {
        let digits = if digits.is_finite() {
            digits.clamp(-300.0, 300.0)
        } else {
            0.0
        };
        let scale = 10f64.powf(digits);
        let scaled = num.magnitude * scale;
        let out = if scaled.is_finite() {
            let stepped = if truncate {
                scaled.trunc()
            } else {
                scaled.round()
            };
            stepped / scale
        } else {
            // The scale overflowed; the value is already exact at this
            // precision.
            num.magnitude
        };
        self.num_value(out, num.family)
    }

    /// `CEILING` (away from zero) and `FLOOR` (toward zero) to a
    /// multiple.
    fn stepped(&mut self, num: &Num, multiple: &Num, away: bool) -> Value {
        if multiple.magnitude == 0.0 {
            return Value::Err(ErrorCode::Div0);
        }
        let steps = num.magnitude.abs() / multiple.magnitude.abs();
        let steps = if away { steps.ceil() } else { steps.floor() };
        let out = steps * multiple.magnitude.abs();
        let out = if num.magnitude < 0.0 { -out } else { out };
        self.num_value(out, num.family)
    }

    fn modulus(&mut self, num: &Num, divisor: &Num) -> Value {
        if divisor.magnitude == 0.0 {
            return Value::Err(ErrorCode::Div0);
        }
        let Some(family) = Self::merged_family(num.family, divisor.family) else {
            return Value::Err(ErrorCode::Value);
        };
        let out = num.magnitude - divisor.magnitude * (num.magnitude / divisor.magnitude).trunc();
        self.num_value(out, family)
    }

    /// The radians measure of an angle operand. A plain number counts as
    /// a radians measure (Visio's internal angle unit).
    fn angle_arg(&mut self, value: &Value) -> Option<f64> {
        let num = self.coerce_num(value);
        match num.family {
            None | Some(Family::Angle) => Some(num.magnitude),
            Some(_) => None,
        }
    }

    fn rgb(&mut self, args: &[Value]) -> Value {
        let mut components = [0u8; 3];
        for (slot, arg) in components.iter_mut().zip(args.iter()) {
            let num = self.coerce_num(arg);
            if num.family.is_some() {
                return Value::Err(ErrorCode::Value);
            }
            let rounded = num.magnitude.round();
            if !(0.0..=255.0).contains(&rounded) {
                return Value::Err(ErrorCode::Num);
            }
            *slot = rounded as u8;
        }
        Value::Color(Color::new(
            components.first().copied().unwrap_or(0),
            components.get(1).copied().unwrap_or(0),
            components.get(2).copied().unwrap_or(0),
        ))
    }

    fn index(&mut self, args: &[Value]) -> Value {
        let position = self.coerce_num(&arg_at(args, 0));
        let magnitude = position.magnitude;
        if position.family.is_some()
            || magnitude.fract() != 0.0
            || !(0.0..=1e15).contains(&magnitude)
        {
            // A bad index is `#VALUE!`, never a panic (ADR 0060).
            return Value::Err(ErrorCode::Value);
        }
        let position = magnitude as usize;
        let list = arg_at(args, 1);
        let delimiter = match args.get(2) {
            Some(value) => text_of(value),
            None => String::from(";"),
        };
        match &list {
            Value::List(items) => items
                .get(position)
                .cloned()
                .unwrap_or_else(|| self.index_default(args)),
            _ => {
                let text = text_of(&list);
                match split_list(&text, &delimiter).get(position) {
                    Some(piece) => Value::Str(String::from(*piece)),
                    None => self.index_default(args),
                }
            }
        }
    }

    /// The value a missing [`Self::index`] position returns: the
    /// `errorvalue` argument when given, the empty string otherwise (the
    /// ShapeSheet reference's documented default).
    fn index_default(&self, args: &[Value]) -> Value {
        match args.get(3) {
            Some(value) => value.clone(),
            None => Value::Str(String::new()),
        }
    }

    fn lookup(&mut self, args: &[Value]) -> Value {
        let key = text_of(&arg_at(args, 0));
        let list = text_of(&arg_at(args, 1));
        let delimiter = match args.get(2) {
            Some(value) => text_of(value),
            None => String::from(";"),
        };
        if delimiter.is_empty() || key.contains(&delimiter) {
            // A key that contains the delimiter cannot name an element.
            return Value::Number(Number::new(-1.0, Unit::Unitsless));
        }
        let found = split_list(&list, &delimiter)
            .iter()
            .position(|piece| key.eq_ignore_ascii_case(piece));
        Value::Number(Number::new(
            found.map_or(-1.0, |position| position as f64),
            Unit::Unitsless,
        ))
    }

    /// Error values propagate and a non-finite number is `#NUM!` at the
    /// source (ADR 0052); both short-circuit the caller. `Some` carries
    /// the value to return instead of computing.
    fn blocked(&mut self, value: &Value) -> Option<Value> {
        match value {
            Value::Err(code) => Some(Value::Err(*code)),
            Value::Number(number) if !number.magnitude.is_finite() => {
                self.warn(EvalWarningKind::NonFinite);
                Some(Value::Err(ErrorCode::Num))
            }
            _ => None,
        }
    }

    /// Coerces a value to a numeric operand through [`crate::coerce::coerce`]
    /// (ADR 0052): the magnitude in the family's canonical unit. A value
    /// that cannot convert becomes zero with one
    /// [`EvalWarningKind::Coercion`] warning.
    fn coerce_num(&mut self, value: &Value) -> Num {
        let coerced = coerce(value, ValueKind::Number);
        if coerced.defaulted {
            self.warn(EvalWarningKind::Coercion(String::from(
                "value defaults to zero",
            )));
            return Num {
                magnitude: 0.0,
                family: None,
            };
        }
        match coerced.value {
            Value::Number(number) => Num {
                // `to_canonical` cannot fail (ADR 0052).
                magnitude: number
                    .unit
                    .to_canonical(number.magnitude)
                    .unwrap_or(number.magnitude),
                family: family_of(number.unit),
            },
            // `coerce` passes an error value through; the callers filter
            // those first, so only a non-finite numeric string lands
            // here. Its `NaN` makes every computation non-finite, which
            // `number_result` turns into `#NUM!`.
            _ => Num {
                magnitude: f64::NAN,
                family: None,
            },
        }
    }

    /// Coerces a value to a boolean through [`crate::coerce::coerce`]
    /// (ADR 0052: zero is `FALSE`; anything that cannot convert is
    /// `FALSE` with one [`EvalWarningKind::Coercion`] warning).
    fn coerce_bool(&mut self, value: &Value) -> bool {
        let coerced = coerce(value, ValueKind::Bool);
        if coerced.defaulted {
            self.warn(EvalWarningKind::Coercion(String::from(
                "value defaults to FALSE",
            )));
        }
        match coerced.value {
            Value::Bool(flag) => flag,
            _ => false,
        }
    }

    /// A computed number: `#NUM!` when the magnitude is not finite
    /// (ADR 0052, P3-11).
    fn num_value(&mut self, magnitude: f64, family: Option<Family>) -> Value {
        let unit = family.map_or(Unit::Unitsless, Family::canonical_unit);
        self.number_result(magnitude, unit)
    }

    fn number_result(&mut self, magnitude: f64, unit: Unit) -> Value {
        if !magnitude.is_finite() {
            self.warn(EvalWarningKind::NonFinite);
            return Value::Err(ErrorCode::Num);
        }
        Value::Number(Number::new(magnitude, unit))
    }
}

/// The argument at `index`, or an error value when the call passed
/// fewer (the arity check runs first; this never panics).
fn arg_at(args: &[Value], index: usize) -> Value {
    args.get(index)
        .cloned()
        .unwrap_or(Value::Err(ErrorCode::Value))
}

/// Splits a list string on `delimiter`, keeping the null strings a
/// leading, trailing or doubled delimiter implies (the ShapeSheet
/// reference's list rule).
fn split_list<'a>(text: &'a str, delimiter: &str) -> Vec<&'a str> {
    if delimiter.is_empty() {
        return vec![text];
    }
    text.split(delimiter).collect()
}

/// The text of a value for `&`, `STRSAME` and the list functions: the
/// one number-to-string conversion of ADR 0052, through
/// [`crate::coerce::coerce`] to [`ValueKind::Str`].
fn text_of(value: &Value) -> String {
    match coerce(value, ValueKind::Str).value {
        Value::Str(text) => text,
        Value::Err(code) => String::from(code.as_str()),
        // `coerce` to a string yields one of the two arms above.
        _ => String::new(),
    }
}

/// The computation family of a unit (ADR 0052): typographic units
/// compute as lengths (points for display, inches for computation) and a
/// percent computes as a plain fraction.
fn family_of(unit: Unit) -> Option<Family> {
    match unit.family() {
        Ok(UnitFamily::Length) | Ok(UnitFamily::Typographic) => Some(Family::Length),
        Ok(UnitFamily::Angle) => Some(Family::Angle),
        Ok(UnitFamily::Duration) => Some(Family::Duration),
        Ok(UnitFamily::Ratio) | Ok(UnitFamily::Dimensionless) => None,
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Span;
    use crate::ast::ExprKind;

    fn sheet() -> SheetRef {
        SheetRef::Page(larrygram_model::PageId { index: 0 })
    }

    struct NullContext;

    impl EvalContext for NullContext {
        fn cell_value(&mut self, _sheet: SheetRef, _reference: &CellRef) -> Result<Value, Error> {
            Ok(Value::Err(ErrorCode::Ref))
        }

        fn set_cell(
            &mut self,
            _sheet: SheetRef,
            _reference: &CellRef,
            _value: Value,
        ) -> Result<(), Error> {
            Ok(())
        }

        fn warn(&mut self, _warning: EvalWarning) {}
    }

    fn literal(literal: Literal) -> Expr {
        Expr {
            span: Span::new(0, 1),
            kind: ExprKind::Literal(literal),
        }
    }

    #[test]
    fn eval_reads_a_literal() {
        let mut ctx = NullContext;
        let expr = literal(Literal::Number(NumberLiteral {
            value: 2.0,
            unit: Some(String::from("in")),
            flag: None,
        }));
        assert_eq!(
            eval(&expr, sheet(), &mut ctx),
            Ok(Value::Number(Number::new(2.0, Unit::Inches)))
        );
    }

    #[test]
    fn a_literal_keeps_the_unit_it_was_written_with() {
        let mut ctx = NullContext;
        let expr = literal(Literal::Number(NumberLiteral {
            value: 37.0,
            unit: Some(String::from("%")),
            flag: None,
        }));
        assert_eq!(
            eval(&expr, sheet(), &mut ctx),
            Ok(Value::Number(Number::new(37.0, Unit::Percent)))
        );
    }

    #[test]
    fn eval_formula_reports_a_parse_failure_through_the_error() {
        // `parse` is P6-02's; the wrapper forwards its result either way.
        let mut ctx = NullContext;
        let out = eval_formula(&Formula::new("1+1"), sheet(), &mut ctx);
        assert!(matches!(out, Ok(_) | Err(Error::NotImplemented("parse"))));
    }

    #[test]
    fn a_number_literal_reads_its_unit_through_the_unit_table() {
        let mut ctx = NullContext;
        let expr = literal(Literal::Number(NumberLiteral {
            value: 5.0,
            unit: Some(String::from("ft")),
            flag: Some('d'),
        }));
        assert_eq!(
            eval(&expr, sheet(), &mut ctx),
            Ok(Value::Number(Number::new(5.0, Unit::Feet)))
        );
    }

    #[test]
    fn families_pick_the_canonical_unit() {
        assert_eq!(Family::Length.canonical_unit(), Unit::Inches);
        assert_eq!(Family::Angle.canonical_unit(), Unit::Radians);
        assert_eq!(Family::Duration.canonical_unit(), Unit::ElapsedSeconds);
        assert_eq!(family_of(Unit::Points), Some(Family::Length));
        assert_eq!(family_of(Unit::Percent), None);
        assert_eq!(family_of(Unit::Radians), Some(Family::Angle));
        assert_eq!(family_of(Unit::ElapsedDays), Some(Family::Duration));
    }

    #[test]
    fn text_of_follows_the_one_string_conversion() {
        assert_eq!(
            text_of(&Value::Number(Number::new(10.0, Unit::Unitsless))),
            "10"
        );
        assert_eq!(text_of(&Value::Bool(true)), "TRUE");
        assert_eq!(text_of(&Value::Color(Color::new(0, 0, 255))), "#0000FF");
        assert_eq!(text_of(&Value::Err(ErrorCode::Value)), "#VALUE!");
    }

    #[test]
    fn a_deeper_tree_than_the_bound_degrades_instead_of_overflowing() {
        let mut expr = literal(Literal::Bool(true));
        for _ in 0..(MAX_EVAL_DEPTH + 2) {
            expr = Expr {
                span: Span::new(0, 1),
                kind: ExprKind::Unary {
                    op: UnaryOp::Minus,
                    operand: Box::new(expr),
                },
            };
        }
        let mut ctx = NullContext;
        assert_eq!(
            eval(&expr, sheet(), &mut ctx),
            Ok(Value::Err(ErrorCode::Value))
        );
    }

    #[test]
    fn warning_kinds_cover_the_evaluator_events() {
        let kinds = [
            EvalWarningKind::UnknownFunction(String::from("THEMEVAL")),
            EvalWarningKind::UnsupportedReference(String::from("Pages[1]!Width")),
            EvalWarningKind::Coercion(String::from("defaults to zero")),
            EvalWarningKind::NonFinite,
            EvalWarningKind::WrongArgumentCount {
                function: String::from("IF"),
                min: 3,
                max: Some(3),
                got: 2,
            },
            EvalWarningKind::SetfRefused(String::from("PinX")),
            EvalWarningKind::Cycle,
        ];
        assert_eq!(kinds.len(), 7);
    }
}
