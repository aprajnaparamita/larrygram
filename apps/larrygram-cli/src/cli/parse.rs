//! Argument parsing and the help text (ADR 0026).
//!
//! The parser is `lexopt`: small, pure Rust, and it leaves the help text
//! to us (AGENTS.md section 4: our own descriptions). `Ok(None)` means
//! `--help` was requested and [`USAGE_TEXT`] has been written to the given
//! stream.

use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};

use lexopt::{Arg, Parser};

use crate::cli::{Background, Command, Export, Inspect, OutputFormat, Render, Usage};

/// The usage and help text of the CLI, written by us (AGENTS.md section 4).
///
/// `--help` writes it to stdout; a usage error writes it to stderr after
/// the error message (ADR 0026).
pub const USAGE_TEXT: &str = "\
larrygram-cli - inspect, render and export Visio drawings

Usage:
  larrygram-cli inspect <file> [--json] [--page N]
  larrygram-cli render <file> --out <path> [--page N] [--format png|svg]
    [--scale PPI] [--background #RRGGBB|none] [--strict]
  larrygram-cli export <file> --out <dir> [--format png|svg] [--scale PPI]
    [--strict]
  larrygram-cli --help

Commands:
  inspect   Summarise a document: kind, pages, masters, styles, themes.
  render    Render one page to a PNG or SVG file.
  export    Render every page of a document into a directory.

Options:
  --json            Print the inspect summary as one JSON object.
  --page N          Select one page, counted from 1 (default: every page
                    for inspect, the first page for render).
  --out PATH        The output file (render) or directory (export).
  --format FORMAT   The output format, png or svg (default: the output
                    file extension, png for export).
  --scale PPI       The device scale in pixels per inch (default: 96).
  --background C    The render page background: a #RRGGBB colour or none
                    for transparent (default: the page background).
  --strict          Exit with the warnings code when warnings occurred.
  -h, --help        Print this help.

Input files: .vsdx, .vssx, .vstx or the binary .vsd, .vss, .vst
(read-only formats, imported into the model).

Exit codes:
  0  success
  1  usage error (bad arguments)
  2  input error (missing, unreadable or unparsable file)
  3  render or output error
  4  warnings occurred and --strict is set
";

/// Parses argv (without `argv[0]`), writing `--help` to `out`.
pub(crate) fn parse_to(
    args: impl Iterator<Item = OsString>,
    out: &mut dyn Write,
) -> Result<Option<Command>, Usage> {
    let mut parser = Parser::from_args(args);
    let Some(arg) = parser.next().map_err(lexop_usage)? else {
        return Err(usage_error(
            "missing command; expected inspect, render or export",
        ));
    };
    let name = match arg {
        Arg::Short('h') | Arg::Long("help") => return write_help(out),
        Arg::Value(value) => value,
        other => return Err(unknown_option(&other)),
    };
    let name = name
        .into_string()
        .map_err(|_| usage_error("the command name is not valid text"))?;
    match name.as_str() {
        "inspect" => parse_inspect(&mut parser, out),
        "render" => parse_render(&mut parser, out),
        "export" => parse_export(&mut parser, out),
        other => Err(usage_error(format!(
            "unknown command: {other}; expected inspect, render or export"
        ))),
    }
}

/// Parses `inspect <file> [--json] [--page N]`.
fn parse_inspect(parser: &mut Parser, out: &mut dyn Write) -> Result<Option<Command>, Usage> {
    let mut file: Option<PathBuf> = None;
    let mut json = false;
    let mut page: Option<u32> = None;
    while let Some(arg) = parser.next().map_err(lexop_usage)? {
        match arg {
            Arg::Short('h') | Arg::Long("help") => return write_help(out),
            Arg::Long("json") => json = true,
            Arg::Long("page") => page = Some(parse_page(&next_value(parser, "page")?)?),
            Arg::Value(value) => {
                if file.is_some() {
                    return Err(usage_error(format!(
                        "unexpected argument: {}",
                        value.to_string_lossy()
                    )));
                }
                file = Some(PathBuf::from(value));
            }
            other => return Err(unknown_option(&other)),
        }
    }
    let file = file.ok_or_else(|| usage_error("missing input file"))?;
    Ok(Some(Command::Inspect(Inspect { file, json, page })))
}

/// Parses `render <file> --out <path> [...]`.
fn parse_render(parser: &mut Parser, out: &mut dyn Write) -> Result<Option<Command>, Usage> {
    let mut file: Option<PathBuf> = None;
    let mut out_path: Option<PathBuf> = None;
    let mut page: Option<u32> = None;
    let mut format: Option<OutputFormat> = None;
    let mut scale: Option<f64> = None;
    let mut background: Option<Background> = None;
    let mut strict = false;
    while let Some(arg) = parser.next().map_err(lexop_usage)? {
        match arg {
            Arg::Short('h') | Arg::Long("help") => return write_help(out),
            Arg::Long("out") => out_path = Some(PathBuf::from(next_value(parser, "out")?)),
            Arg::Long("page") => page = Some(parse_page(&next_value(parser, "page")?)?),
            Arg::Long("format") => format = Some(parse_format(&next_value(parser, "format")?)?),
            Arg::Long("scale") => scale = Some(parse_scale(&next_value(parser, "scale")?)?),
            Arg::Long("background") => {
                background = Some(parse_background(&next_value(parser, "background")?)?);
            }
            Arg::Long("strict") => strict = true,
            Arg::Value(value) => {
                if file.is_some() {
                    return Err(usage_error(format!(
                        "unexpected argument: {}",
                        value.to_string_lossy()
                    )));
                }
                file = Some(PathBuf::from(value));
            }
            other => return Err(unknown_option(&other)),
        }
    }
    let file = file.ok_or_else(|| usage_error("missing input file"))?;
    let out = out_path.ok_or_else(|| usage_error("missing output file; use --out"))?;
    let format = match format {
        Some(format) => format,
        None => format_from_extension(&out).ok_or_else(|| {
            usage_error(format!(
                "cannot tell the output format from {}; use --format",
                out.display()
            ))
        })?,
    };
    let scale = scale.unwrap_or(super::exec::DEFAULT_SCALE);
    Ok(Some(Command::Render(Render {
        file,
        out,
        page,
        format,
        scale,
        background,
        strict,
    })))
}

/// Parses `export <file> --out <dir> [...]`.
fn parse_export(parser: &mut Parser, out: &mut dyn Write) -> Result<Option<Command>, Usage> {
    let mut file: Option<PathBuf> = None;
    let mut out_path: Option<PathBuf> = None;
    let mut format: Option<OutputFormat> = None;
    let mut scale: Option<f64> = None;
    let mut strict = false;
    while let Some(arg) = parser.next().map_err(lexop_usage)? {
        match arg {
            Arg::Short('h') | Arg::Long("help") => return write_help(out),
            Arg::Long("out") => out_path = Some(PathBuf::from(next_value(parser, "out")?)),
            Arg::Long("format") => format = Some(parse_format(&next_value(parser, "format")?)?),
            Arg::Long("scale") => scale = Some(parse_scale(&next_value(parser, "scale")?)?),
            Arg::Long("strict") => strict = true,
            Arg::Value(value) => {
                if file.is_some() {
                    return Err(usage_error(format!(
                        "unexpected argument: {}",
                        value.to_string_lossy()
                    )));
                }
                file = Some(PathBuf::from(value));
            }
            other => return Err(unknown_option(&other)),
        }
    }
    let file = file.ok_or_else(|| usage_error("missing input file"))?;
    let out = out_path.ok_or_else(|| usage_error("missing output directory; use --out"))?;
    let format = format.unwrap_or(OutputFormat::Png);
    let scale = scale.unwrap_or(super::exec::DEFAULT_SCALE);
    Ok(Some(Command::Export(Export {
        file,
        out,
        format,
        scale,
        strict,
    })))
}

/// The value of the option being parsed, as text.
fn next_value(parser: &mut Parser, name: &str) -> Result<String, Usage> {
    let value = parser.value().map_err(lexop_usage)?;
    value
        .into_string()
        .map_err(|_| usage_error(format!("the value of --{name} is not valid text")))
}

/// The `--page` value: a page number counted from 1.
fn parse_page(text: &str) -> Result<u32, Usage> {
    let number: u32 = text
        .parse()
        .map_err(|_| usage_error(format!("invalid page number: {text}")))?;
    if number == 0 {
        return Err(usage_error("page numbers start at 1"));
    }
    Ok(number)
}

/// The `--scale` value: pixels per inch, finite and positive (ADR 0025).
fn parse_scale(text: &str) -> Result<f64, Usage> {
    let scale: f64 = text
        .parse()
        .map_err(|_| usage_error(format!("invalid scale: {text}")))?;
    if !scale.is_finite() || scale <= 0.0 {
        return Err(usage_error(format!(
            "scale must be a positive number: {text}"
        )));
    }
    Ok(scale)
}

/// The `--format` value: `png` or `svg` (ADR 0026).
fn parse_format(text: &str) -> Result<OutputFormat, Usage> {
    match text {
        "png" => Ok(OutputFormat::Png),
        "svg" => Ok(OutputFormat::Svg),
        other => Err(usage_error(format!(
            "unknown format: {other}; expected png or svg"
        ))),
    }
}

/// The `--background` value: `none` or `#RRGGBB` (ADR 0026).
fn parse_background(text: &str) -> Result<Background, Usage> {
    if text == "none" {
        return Ok(Background::Transparent);
    }
    let hex = text.strip_prefix('#').unwrap_or(text);
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(usage_error(format!(
            "invalid background: {text}; use #RRGGBB or none"
        )));
    }
    let channel = |from: usize| -> u8 {
        u8::from_str_radix(hex.get(from..from + 2).unwrap_or("0"), 16).unwrap_or(0)
    };
    Ok(Background::Color(larrygram_render::Rgba::opaque(
        channel(0),
        channel(2),
        channel(4),
    )))
}

/// The output format an output file extension names, if any.
fn format_from_extension(out: &Path) -> Option<OutputFormat> {
    match out
        .extension()
        .and_then(|e| e.to_str())?
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => Some(OutputFormat::Png),
        "svg" => Some(OutputFormat::Svg),
        _ => None,
    }
}

/// Writes [`USAGE_TEXT`] to `out`.
fn write_help(out: &mut dyn Write) -> Result<Option<Command>, Usage> {
    out.write_all(USAGE_TEXT.as_bytes())
        .and_then(|()| out.flush())
        .map_err(|error| usage_error(format!("cannot write help text: {error}")))?;
    Ok(None)
}

/// A usage error with a message for stderr.
fn usage_error(message: impl Into<String>) -> Usage {
    Usage {
        message: message.into(),
    }
}

/// The usage error of an argument `lexopt` rejected or did not give.
fn lexop_usage(error: lexopt::Error) -> Usage {
    usage_error(error.to_string())
}

/// The usage error of an argument we do not know.
fn unknown_option(arg: &Arg<'_>) -> Usage {
    let name = match arg {
        Arg::Short(c) => format!("-{c}"),
        Arg::Long(name) => format!("--{name}"),
        Arg::Value(value) => value.to_string_lossy().into_owned(),
    };
    usage_error(format!("unknown option: {name}"))
}

#[cfg(test)]
mod tests {
    use larrygram_render::Rgba;

    use super::*;

    fn parse(args: &[&str]) -> Result<Option<Command>, Usage> {
        parse_to(args.iter().map(|a| OsString::from(*a)), &mut Vec::new())
    }

    fn command(args: &[&str]) -> Command {
        match parse(args) {
            Ok(Some(command)) => command,
            other => panic!("expected a command, got {other:?}"),
        }
    }

    fn as_inspect(command: Command) -> Inspect {
        match command {
            Command::Inspect(i) => i,
            other => panic!("expected inspect, got {other:?}"),
        }
    }

    fn as_render(command: Command) -> Render {
        match command {
            Command::Render(r) => r,
            other => panic!("expected render, got {other:?}"),
        }
    }

    fn as_export(command: Command) -> Export {
        match command {
            Command::Export(e) => e,
            other => panic!("expected export, got {other:?}"),
        }
    }

    fn error(args: &[&str]) -> String {
        match parse(args) {
            Err(usage) => usage.message,
            other => panic!("expected a usage error, got {other:?}"),
        }
    }

    #[test]
    fn inspect_parses_its_arguments() {
        let first = command(&["inspect", "a.vsdx"]);
        let i = as_inspect(first);
        assert_eq!(i.file, PathBuf::from("a.vsdx"));
        assert!(!i.json);
        assert_eq!(i.page, None);

        let second = command(&["inspect", "a.vsdx", "--json", "--page", "2"]);
        let i = as_inspect(second);
        assert!(i.json);
        assert_eq!(i.page, Some(2));
    }

    #[test]
    fn render_parses_its_arguments() {
        let first = command(&["render", "a.vsdx", "--out", "a.png"]);
        let r = as_render(first);
        assert_eq!(r.out, PathBuf::from("a.png"));
        assert_eq!(r.page, None);
        assert_eq!(r.format, OutputFormat::Png);
        assert_eq!(r.scale, crate::cli::exec::DEFAULT_SCALE);
        assert_eq!(r.background, None);
        assert!(!r.strict);

        let second = command(&[
            "render",
            "a.vsdx",
            "--out",
            "a.svg",
            "--page",
            "3",
            "--format",
            "png",
            "--scale",
            "300",
            "--background",
            "#102030",
            "--strict",
        ]);
        let r = as_render(second);
        assert_eq!(r.page, Some(3));
        assert_eq!(r.format, OutputFormat::Png);
        assert_eq!(r.scale, 300.0);
        assert_eq!(
            r.background,
            Some(Background::Color(Rgba::opaque(0x10, 0x20, 0x30)))
        );
        assert!(r.strict);
    }

    #[test]
    fn background_none_is_transparent() {
        let transparent = command(&["render", "a.vsdx", "--out", "a.png", "--background", "none"]);
        let r = as_render(transparent);
        assert_eq!(r.background, Some(Background::Transparent));
    }

    #[test]
    fn export_parses_its_arguments() {
        let first = command(&["export", "a.vsdx", "--out", "out"]);
        let e = as_export(first);
        assert_eq!(e.out, PathBuf::from("out"));
        assert_eq!(e.format, OutputFormat::Png);
        assert_eq!(e.scale, crate::cli::exec::DEFAULT_SCALE);
        assert!(!e.strict);

        let second = command(&[
            "export", "a.vsdx", "--out", "out", "--format", "svg", "--scale", "200", "--strict",
        ]);
        let e = as_export(second);
        assert_eq!(e.format, OutputFormat::Svg);
        assert_eq!(e.scale, 200.0);
        assert!(e.strict);
    }

    #[test]
    fn format_comes_from_the_extension() {
        let uppercase = command(&["render", "a.vsdx", "--out", "dir/a.SVG"]);
        let r = as_render(uppercase);
        assert_eq!(r.format, OutputFormat::Svg);
    }

    #[test]
    fn help_writes_the_usage_text() {
        let mut out = Vec::new();
        let result = parse_to(["--help"].map(OsString::from).into_iter(), &mut out);
        assert_eq!(result, Ok(None));
        assert_eq!(String::from_utf8_lossy(&out), USAGE_TEXT);
        let mut out = Vec::new();
        let result = parse_to(
            ["render", "--help"].map(OsString::from).into_iter(),
            &mut out,
        );
        assert_eq!(result, Ok(None));
        assert_eq!(String::from_utf8_lossy(&out), USAGE_TEXT);
    }

    #[test]
    fn missing_command_is_a_usage_error() {
        assert_eq!(
            error(&[]),
            "missing command; expected inspect, render or export"
        );
    }

    #[test]
    fn unknown_command_is_a_usage_error() {
        assert_eq!(
            error(&["frobnicate", "a.vsdx"]),
            "unknown command: frobnicate; expected inspect, render or export"
        );
    }

    #[test]
    fn unknown_options_are_usage_errors() {
        assert_eq!(
            error(&["inspect", "a.vsdx", "--bogus"]),
            "unknown option: --bogus"
        );
        assert_eq!(error(&["inspect", "-x"]), "unknown option: -x");
        assert_eq!(error(&["--bogus"]), "unknown option: --bogus");
    }

    #[test]
    fn missing_file_or_out_is_a_usage_error() {
        assert_eq!(error(&["inspect"]), "missing input file");
        assert_eq!(
            error(&["render", "a.vsdx"]),
            "missing output file; use --out"
        );
        assert_eq!(
            error(&["export", "a.vsdx"]),
            "missing output directory; use --out"
        );
    }

    #[test]
    fn a_second_positional_is_a_usage_error() {
        assert_eq!(
            error(&["inspect", "a.vsdx", "b.vsdx"]),
            "unexpected argument: b.vsdx"
        );
    }

    #[test]
    fn bad_option_values_are_usage_errors() {
        assert_eq!(
            error(&["inspect", "a.vsdx", "--page", "0"]),
            "page numbers start at 1"
        );
        assert_eq!(
            error(&["inspect", "a.vsdx", "--page", "abc"]),
            "invalid page number: abc"
        );
        assert_eq!(
            error(&["inspect", "a.vsdx", "--page"]),
            "missing argument for option '--page'"
        );
        assert_eq!(
            error(&["render", "a.vsdx", "--out", "a.png", "--scale", "0"]),
            "scale must be a positive number: 0"
        );
        assert_eq!(
            error(&["render", "a.vsdx", "--out", "a.png", "--scale", "nan"]),
            "scale must be a positive number: nan"
        );
        assert_eq!(
            error(&["export", "a.vsdx", "--out", "d", "--format", "gif"]),
            "unknown format: gif; expected png or svg"
        );
        assert_eq!(
            error(&["render", "a.vsdx", "--out", "a.png", "--background", "red"]),
            "invalid background: red; use #RRGGBB or none"
        );
    }

    #[test]
    fn page_numbers_stay_in_their_bounds() {
        // The largest page number parses; one past it does not.
        let max = command(&["inspect", "a.vsdx", "--page", "4294967295"]);
        let i = as_inspect(max);
        assert_eq!(i.page, Some(u32::MAX));
        assert_eq!(
            error(&["inspect", "a.vsdx", "--page", "4294967296"]),
            "invalid page number: 4294967296"
        );
        assert_eq!(
            error(&["inspect", "a.vsdx", "--page", "-1"]),
            "invalid page number: -1"
        );
        assert_eq!(
            error(&["inspect", "a.vsdx", "--page", ""]),
            "invalid page number: "
        );
        assert_eq!(
            error(&["inspect", "a.vsdx", "--page", " 2"]),
            "invalid page number:  2"
        );
        // A leading plus is accepted by the integer parser; pinned.
        let i = as_inspect(command(&["inspect", "a.vsdx", "--page", "+2"]));
        assert_eq!(i.page, Some(2));
    }

    #[test]
    fn scale_values_stay_in_their_bounds() {
        // Finite and positive only: zero, negatives and infinities out.
        for bad in ["0", "-1", "inf", "-inf", "1e400", "nan", "NaN"] {
            assert_eq!(
                error(&["render", "a.vsdx", "--out", "a.png", "--scale", bad]),
                format!("scale must be a positive number: {bad}"),
                "{bad}"
            );
        }
        for bad in ["", " 1", "x"] {
            assert_eq!(
                error(&["render", "a.vsdx", "--out", "a.png", "--scale", bad]),
                format!("invalid scale: {bad}"),
                "{bad:?}"
            );
        }
        for good in ["1", "0.5", "1e-300", "300.5"] {
            let r = as_render(command(&[
                "render", "a.vsdx", "--out", "a.png", "--scale", good,
            ]));
            assert!(r.scale.is_finite() && r.scale > 0.0, "{good}");
        }
    }

    #[test]
    fn format_and_background_values_are_exact() {
        // The format and the `none` keyword are case-sensitive.
        assert_eq!(
            error(&["render", "a.vsdx", "--out", "a.png", "--format", "PNG"]),
            "unknown format: PNG; expected png or svg"
        );
        assert_eq!(
            error(&["render", "a.vsdx", "--out", "a.png", "--background", "NONE"]),
            "invalid background: NONE; use #RRGGBB or none"
        );
        assert_eq!(
            error(&["render", "a.vsdx", "--out", "a.png", "--background", ""]),
            "invalid background: ; use #RRGGBB or none"
        );
        assert_eq!(
            error(&[
                "render",
                "a.vsdx",
                "--out",
                "a.png",
                "--background",
                "##123456"
            ]),
            "invalid background: ##123456; use #RRGGBB or none"
        );
        // Uppercase hex is fine, with or without the hash.
        for good in ["#ABCDEF", "abcdef"] {
            let r = as_render(command(&[
                "render",
                "a.vsdx",
                "--out",
                "a.png",
                "--background",
                good,
            ]));
            assert_eq!(
                r.background,
                Some(Background::Color(Rgba::opaque(0xAB, 0xCD, 0xEF))),
                "{good}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_arguments_are_usage_errors() {
        use std::os::unix::ffi::OsStringExt;
        let mut out = Vec::new();
        let bogus = OsString::from_vec(vec![0xff]);
        let result = parse_to([bogus.clone()].into_iter(), &mut out);
        assert_eq!(
            result.unwrap_err().to_string(),
            "the command name is not valid text"
        );
        let result = parse_to(
            [
                OsString::from("render"),
                OsString::from("a.vsdx"),
                OsString::from("--out"),
                bogus,
            ]
            .into_iter(),
            &mut out,
        );
        assert_eq!(
            result.unwrap_err().to_string(),
            "the value of --out is not valid text"
        );
    }

    #[test]
    fn render_without_a_known_extension_needs_a_format() {
        assert_eq!(
            error(&["render", "a.vsdx", "--out", "a"]),
            "cannot tell the output format from a; use --format"
        );
        let with_format = command(&["render", "a.vsdx", "--out", "a", "--format", "svg"]);
        let r = as_render(with_format);
        assert_eq!(r.format, OutputFormat::Svg);
    }

    #[test]
    fn background_accepts_hex_without_the_hash() {
        assert_eq!(
            parse_background("102030"),
            Ok(Background::Color(Rgba::opaque(0x10, 0x20, 0x30)))
        );
        assert!(parse_background("#10203").is_err());
        assert!(parse_background("#1020300").is_err());
        assert!(parse_background("#1020zz").is_err());
    }

    #[test]
    fn options_may_follow_a_double_dash_value() {
        let dashed = command(&["inspect", "--", "--odd-name.vsdx"]);
        let i = as_inspect(dashed);
        assert_eq!(i.file, PathBuf::from("--odd-name.vsdx"));
    }

    #[test]
    fn usage_error_codes_are_strings_for_stderr() {
        let usage = usage_error("bad");
        assert_eq!(usage.to_string(), "bad");
    }

    #[test]
    fn render_and_export_reject_extra_arguments_and_unknown_options() {
        assert_eq!(
            error(&["render", "a.vsdx", "--out", "a.png", "extra.vsdx"]),
            "unexpected argument: extra.vsdx"
        );
        assert_eq!(
            error(&["render", "a.vsdx", "--out", "a.png", "--bogus"]),
            "unknown option: --bogus"
        );
        assert_eq!(
            error(&["export", "a.vsdx", "--out", "out", "extra.vsdx"]),
            "unexpected argument: extra.vsdx"
        );
        assert_eq!(
            error(&["export", "a.vsdx", "--out", "out", "--bogus"]),
            "unknown option: --bogus"
        );
    }

    #[test]
    fn an_unrecognizable_output_extension_is_a_usage_error() {
        assert_eq!(
            error(&["render", "a.vsdx", "--out", "out.xyz"]),
            "cannot tell the output format from out.xyz; use --format"
        );
    }
}
