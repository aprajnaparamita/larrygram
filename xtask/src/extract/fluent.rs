//! Fluent UI command bar extractor (P1-03).
//!
//! Reads `reference/fluent-ui-ids/Office 2016/visiocontrols.xlsx` and
//! `reference/fluent-ui-ids/Microsoft 365/Current Channel/visiocontrols.xlsx`
//! and produces:
//!
//! - `ui-spec/panel-2016.yaml`
//! - `ui-spec/panel-365.yaml`
//!
//! See ADR 0007 for the layout file format and ADR 0009 for the
//! extractor contract.
//!
//! Microsoft 365 source choice: we use the **Current Channel** xlsx
//! (1,161 data rows) per ADR 0007 and the P1-01 source notes. The
//! Monthly Enterprise and Semi-Annual Enterprise Channel xlsx files
//! are present in `reference/` but are not consumed here.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use calamine::{Data, Range, Reader, open_workbook_auto};
use serde_json::{Map, Value, json};

use super::schema::Schema;
use super::util::{emit_yaml, write_and_validate};
use super::{ExtractError, ExtractResult};

/// Hand-written Fluent control -> Larrygram command id bindings
/// (P4-02, ADR 0006 amendment).
///
/// A control listed here surfaces the given command id whatever the
/// mechanical derivation in [`super::commands`] would pick. The rules
/// bind the controls our own commands cover: `ShowRulerVisio` is the
/// Rulers check box (`view.rulers`), `ShowGrid` is the Grid check box
/// (`view.toggle_grid`, the `visCmdViewGrid` id), and
/// `ZoomFitToWindowVisio` is the Fit to Window button (`view.zoom.fit`).
///
/// Every control name must exist in `ui-spec/panel-2016.yaml` or
/// `ui-spec/panel-365.yaml`; [`super::commands`] fails the extraction
/// for a rule that names a control in neither layout. The `FileExit`
/// family of the task has no control in either layout (the Fluent rows
/// are dropped as "Not in the TabBar"), so `app.quit` has no binding
/// here yet.
pub const FLUENT_COMMANDS: &[(&str, &str)] = &[
    ("ShowGrid", "view.toggle_grid"),
    ("ShowRulerVisio", "view.rulers"),
    ("ZoomFitToWindowVisio", "view.zoom.fit"),
];

/// Per-extractor entry point. Reads from `in_dir` (the
/// `reference/` root), writes to `out_dir` (the workspace root), and
/// returns the list of files written.
pub fn run(in_dir: &Path, out_dir: &Path) -> ExtractResult<Vec<PathBuf>> {
    let schema = Schema::load_default()?;

    let office_2016 = in_dir
        .join("fluent-ui-ids")
        .join("Office 2016")
        .join("visiocontrols.xlsx");
    let m365_current = in_dir
        .join("fluent-ui-ids")
        .join("Microsoft 365")
        .join("Current Channel")
        .join("visiocontrols.xlsx");

    // `source_revision` records the pinned upstream identifier so a
    // future Phase can re-run the extractor against a different
    // commit and the diff is obvious. The Fluent data was pinned at
    // commit `b230a0df45036b2d5e8b49ebf368b3f90ada8d63` on
    // 2026-10-12 (P1-01).
    let layout_2016 = build_layout(
        &office_2016,
        "Office Fluent UI 2016, Visio",
        "https://github.com/OfficeDev/office-fluent-ui-command-identifiers",
        "Office 2016, Visio control-identifier spreadsheet (commit b230a0df45036b2d5e8b49ebf368b3f90ada8d63, 2024-03-18)",
    )?;
    let layout_365 = build_layout(
        &m365_current,
        "Microsoft Fluent UI 365, Current Channel, Visio",
        "https://github.com/OfficeDev/office-fluent-ui-command-identifiers",
        "Microsoft 365 Current Channel, Visio control-identifier spreadsheet (commit b230a0df45036b2d5e8b49ebf368b3f90ada8d63, 2024-03-18). 1,161 data rows.",
    )?;

    let path_2016 = out_dir.join("ui-spec").join("panel-2016.yaml");
    let path_365 = out_dir.join("ui-spec").join("panel-365.yaml");
    write_and_validate(&schema, &path_2016, &layout_2016.text)?;
    write_and_validate(&schema, &path_365, &layout_365.text)?;

    Ok(vec![path_2016, path_365])
}

// ---- Row model --------------------------------------------------------

/// One row from the Fluent UI xlsx. All fields are normalised
/// strings (Policy ID is i64). Empty cells become `""`; an empty
/// `Policy ID` cell becomes [`NO_POLICY_ID`] (the source does not
/// order the control) and a non-empty cell that does not parse is an
/// [`ExtractError::Parse`] naming the row.
#[derive(Debug, Clone)]
struct Row {
    /// 1-based row number in the worksheet (header is row 1).
    sheet_row: u32,
    control_name: String,
    control_type: String,
    tab_set: String,
    tab: String,
    group: String,
    parent: String,
    policy_id: i64,
}

/// Sort key for a row whose `Policy ID` cell is empty: the source
/// does not order that control, so it sorts after every numbered
/// control (and never collides with a real parse failure).
const NO_POLICY_ID: i64 = i64::MAX;

impl Row {
    fn section(&self) -> Section {
        match self.tab_set.as_str() {
            "" => Section::Drop,
            "None (Backstage View)" => Section::Backstage,
            "None (Context Menu)" => Section::ContextMenu,
            "None (Core Tab)" => Section::CommandBar,
            "None (Quick Access Toolbar)" => Section::Qat,
            "None (Not in the Ribbon)" => Section::Drop,
            ts if ts.starts_with("TabSet") => Section::CommandBar,
            _ => Section::Drop,
        }
    }

    /// Map the raw Fluent `Control Type` to the schema enum.
    fn mapped_type(&self) -> Option<String> {
        let raw = self.control_type.trim();
        if raw.is_empty() {
            return Some("none".to_owned());
        }
        if let Some(rest) = raw.strip_prefix("None (")
            && rest.ends_with(')')
        {
            return Some("none".to_owned());
        }
        if raw == "button (dialogBoxLauncher)" {
            return Some("buttonDialogBoxLauncher".to_owned());
        }
        if raw.starts_with("button (") {
            // Future-proof: any button (xxx) maps to buttonXxx.
            let inner = raw.trim_start_matches("button (").trim_end_matches(')');
            let mut mapped = String::from("button");
            mapped.push_str(inner);
            return Some(mapped);
        }
        if !is_known_type(raw) {
            return None;
        }
        Some(raw.to_owned())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    Drop,
    CommandBar,
    Backstage,
    Qat,
    ContextMenu,
}

/// Returns true when `raw` matches one of the schema's `controlType`
/// enum values. Called after stripping "None (...)" and
/// "button (xxx)" wrappers.
fn is_known_type(raw: &str) -> bool {
    matches!(
        raw,
        "tab"
            | "tabSet"
            | "group"
            | "button"
            | "toggleButton"
            | "splitButton"
            | "gallery"
            | "menu"
            | "checkBox"
            | "dropDown"
            | "comboBox"
            | "labelControl"
            | "control"
            | "contextMenu"
            | "task"
            | "taskFormGroup"
            | "category"
    )
}

/// One layout file (2016 or 365) together with the rendered YAML.
struct BuiltLayout {
    text: String,
}

fn build_layout(
    xlsx_path: &Path,
    source: &str,
    source_url: &str,
    source_revision: &str,
) -> ExtractResult<BuiltLayout> {
    let rows = parse_xlsx(xlsx_path)?;
    let row_refs: Vec<&Row> = rows.iter().collect();
    let layout = assemble_layout(&row_refs, xlsx_path, source, source_url, source_revision)?;
    let text = emit_yaml(&layout).map_err(|msg| ExtractError::Parse {
        path: xlsx_path.to_path_buf(),
        message: msg,
    })?;
    Ok(BuiltLayout { text })
}

// ---- xlsx parsing ----------------------------------------------------

fn parse_xlsx(path: &Path) -> ExtractResult<Vec<Row>> {
    if !path.is_file() {
        return Err(ExtractError::Missing {
            path: path.to_path_buf(),
        });
    }
    let mut book = open_workbook_auto(path).map_err(|e| ExtractError::Parse {
        path: path.to_path_buf(),
        message: format!("open: {e}"),
    })?;
    let sheet_name = book
        .sheet_names()
        .first()
        .cloned()
        .ok_or_else(|| ExtractError::Parse {
            path: path.to_path_buf(),
            message: "xlsx has no sheets".into(),
        })?;
    let range = book
        .worksheet_range(&sheet_name)
        .map_err(|e| ExtractError::Parse {
            path: path.to_path_buf(),
            message: format!("worksheet {sheet_name}: {e}"),
        })?;
    rows_from_range(&range, path)
}

fn rows_from_range(range: &Range<Data>, path: &Path) -> ExtractResult<Vec<Row>> {
    let header = range.rows().next().ok_or_else(|| ExtractError::Parse {
        path: path.to_path_buf(),
        message: "empty sheet".into(),
    })?;
    let cols = resolve_columns(header, path)?;
    let mut rows = Vec::new();
    for (idx, row) in range.rows().enumerate().skip(1) {
        let sheet_row = (idx as u32) + 1;
        let get = |i: Option<usize>| -> String {
            match i {
                Some(i) if i < row.len() => cell_to_string(&row[i]),
                _ => String::new(),
            }
        };
        let control_name = get(cols.control_name);
        if control_name.is_empty() {
            // Spacer row; skip silently.
            continue;
        }
        // R4: a non-empty Policy ID that does not parse means the
        // source changed under us; an empty cell means "unordered"
        // ([`NO_POLICY_ID`]).
        let policy_raw = get(cols.policy_id);
        let policy_id = if policy_raw.trim().is_empty() {
            NO_POLICY_ID
        } else {
            policy_raw.parse::<i64>().map_err(|_| ExtractError::Parse {
                path: path.to_path_buf(),
                message: format!(
                    "row {sheet_row} (cn={control_name:?}): Policy ID {policy_raw:?} is not an integer"
                ),
            })?
        };
        rows.push(Row {
            sheet_row,
            control_name,
            control_type: get(cols.control_type),
            tab_set: get(cols.tab_set),
            tab: get(cols.tab),
            group: get(cols.group),
            parent: get(cols.parent),
            policy_id,
        });
    }
    Ok(rows)
}

#[derive(Debug, Clone, Copy)]
struct Columns {
    control_name: Option<usize>,
    control_type: Option<usize>,
    tab_set: Option<usize>,
    tab: Option<usize>,
    group: Option<usize>,
    parent: Option<usize>,
    policy_id: Option<usize>,
}

fn resolve_columns(header: &[Data], path: &Path) -> ExtractResult<Columns> {
    let find = |name: &str| -> Option<usize> {
        header
            .iter()
            .position(|c| matches!(c, Data::String(s) if s == name))
    };
    let cols = Columns {
        control_name: find("Control Name"),
        control_type: find("Control Type"),
        tab_set: find("Tab Set"),
        tab: find("Tab"),
        group: find("Group/Context Menu Name"),
        parent: find("Parent Control"),
        policy_id: find("Policy ID"),
    };
    if cols.control_name.is_none()
        || cols.control_type.is_none()
        || cols.tab_set.is_none()
        || cols.tab.is_none()
        || cols.group.is_none()
        || cols.parent.is_none()
        || cols.policy_id.is_none()
    {
        let header_strings: Vec<String> = header.iter().map(cell_to_string).collect();
        return Err(ExtractError::Parse {
            path: path.to_path_buf(),
            message: format!("header missing required columns (got {:?})", header_strings),
        });
    }
    Ok(cols)
}

fn cell_to_string(c: &Data) -> String {
    match c {
        Data::Empty => String::new(),
        Data::String(s) => s.clone(),
        Data::Float(f) => {
            if f.is_finite() && f.fract() == 0.0 {
                format!("{}", *f as i64)
            } else {
                format!("{f}")
            }
        }
        Data::Int(i) => i.to_string(),
        Data::Bool(b) => b.to_string(),
        Data::DateTime(d) => format!("{d:?}"),
        Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
        Data::Error(e) => format!("{e:?}"),
    }
}

// ---- Layout assembly --------------------------------------------------

fn assemble_layout(
    rows: &[&Row],
    xlsx_path: &Path,
    source: &str,
    source_url: &str,
    source_revision: &str,
) -> ExtractResult<Value> {
    // Map every row that has an unknown control type to an error.
    for r in rows {
        if r.mapped_type().is_none() {
            return Err(ExtractError::Parse {
                path: xlsx_path.to_path_buf(),
                message: format!(
                    "row {} (cn={:?}) has unknown control type {:?}",
                    r.sheet_row, r.control_name, r.control_type
                ),
            });
        }
    }

    // Group rows by section.
    let mut command_bar_rows: Vec<&Row> = Vec::new();
    let mut backstage_rows: Vec<&Row> = Vec::new();
    let mut qat_rows: Vec<&Row> = Vec::new();
    let mut context_menu_rows: Vec<&Row> = Vec::new();
    for r in rows {
        match r.section() {
            Section::CommandBar => command_bar_rows.push(r),
            Section::Backstage => backstage_rows.push(r),
            Section::Qat => qat_rows.push(r),
            Section::ContextMenu => context_menu_rows.push(r),
            Section::Drop => {}
        }
    }

    let tabs = build_tabs(&command_bar_rows)?;
    let backstage = build_backstage(&backstage_rows)?;
    let context_menus = build_context_menus(&context_menu_rows)?;
    let quick_access_toolbar = build_qat(&qat_rows)?;

    // Top-level keys are alphabetical via BTreeMap / serde_json.
    let mut top = Map::new();
    top.insert("backstage".into(), backstage);
    top.insert("context_menus".into(), Value::Array(context_menus));
    top.insert("product".into(), Value::String("visio".into()));
    top.insert("quick_access_toolbar".into(), quick_access_toolbar);
    top.insert("schema_version".into(), json!(1));
    top.insert("source".into(), Value::String(source.into()));
    top.insert(
        "source_revision".into(),
        Value::String(source_revision.into()),
    );
    top.insert("source_url".into(), Value::String(source_url.into()));
    top.insert("tabs".into(), Value::Array(tabs));

    Ok(Value::Object(top))
}

fn build_tabs(rows: &[&Row]) -> ExtractResult<Vec<Value>> {
    // Find every "tab" row that is the root of a tab (no parent tab).
    let mut tab_rows: Vec<&Row> = rows
        .iter()
        .copied()
        .filter(|r| r.control_type == "tab" && r.tab.is_empty())
        .collect();
    // Stable order: source row ascending.
    tab_rows.sort_by_key(|r| r.sheet_row);

    // Pre-compute the set of alternate parent group ids for every
    // control name. ADR 0007: when the same control appears in
    // multiple groups (across tabs or within one tab), the
    // additional group ids are recorded in `aliases`. The empty
    // string is not a valid group id, so we skip it.
    let mut alt_groups: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for r in rows {
        if r.control_type == "tab" || r.control_type == "group" {
            continue;
        }
        if r.group.is_empty() {
            continue;
        }
        alt_groups
            .entry(r.control_name.clone())
            .or_default()
            .insert(r.group.clone());
    }

    let mut tabs = Vec::new();
    for (i, tab_row) in tab_rows.iter().enumerate() {
        let tab_set = if tab_row.tab_set.is_empty() {
            Value::Null
        } else {
            Value::String(tab_row.tab_set.clone())
        };
        let groups = build_groups_for_tab(tab_row, rows, &alt_groups)?;
        let mut obj = Map::new();
        obj.insert("bounds".into(), Value::Null);
        obj.insert("groups".into(), Value::Array(groups));
        obj.insert("icon_ref".into(), Value::Null);
        obj.insert("id".into(), Value::String(tab_row.control_name.clone()));
        obj.insert("keytip".into(), Value::Null);
        obj.insert("label".into(), Value::String(tab_row.control_name.clone()));
        obj.insert("order".into(), json!(i as u32));
        obj.insert("size_variant".into(), Value::Null);
        obj.insert("tab_set".into(), tab_set);
        obj.insert("type".into(), Value::String("tab".into()));
        obj.insert("visual_notes".into(), Value::Null);
        tabs.push(Value::Object(obj));
    }
    Ok(tabs)
}

fn build_groups_for_tab(
    tab_row: &Row,
    rows: &[&Row],
    alt_groups: &BTreeMap<String, BTreeSet<String>>,
) -> ExtractResult<Vec<Value>> {
    let tab_name = &tab_row.control_name;
    let mut group_rows: Vec<&Row> = rows
        .iter()
        .copied()
        .filter(|r| r.control_type == "group" && r.tab == *tab_name)
        .collect();
    group_rows.sort_by_key(|r| r.sheet_row);

    let mut groups = Vec::new();
    for (gi, group_row) in group_rows.iter().enumerate() {
        let group_name = &group_row.control_name;
        let mut control_rows: Vec<&Row> = rows
            .iter()
            .copied()
            .filter(|r| {
                r.tab == *tab_name
                    && r.group == *group_name
                    && r.control_type != "tab"
                    && r.control_type != "group"
            })
            .collect();
        // Sort: Policy ID ascending, then Control Name, then source row.
        control_rows.sort_by(|a, b| {
            a.policy_id
                .cmp(&b.policy_id)
                .then_with(|| a.control_name.cmp(&b.control_name))
                .then_with(|| a.sheet_row.cmp(&b.sheet_row))
        });

        let mut controls = Vec::new();
        for (ci, c) in control_rows.iter().enumerate() {
            let mut aliases: Vec<String> = alt_groups
                .get(&c.control_name)
                .map(|s| s.iter().cloned().collect())
                .unwrap_or_default();
            aliases.retain(|g| g != &c.group);
            aliases.sort();
            aliases.dedup();
            controls.push(control_to_value(c, ci, aliases));
        }

        let mut obj = Map::new();
        obj.insert("bounds".into(), Value::Null);
        obj.insert("controls".into(), Value::Array(controls));
        obj.insert("icon_ref".into(), Value::Null);
        obj.insert("id".into(), Value::String(group_name.clone()));
        obj.insert("keytip".into(), Value::Null);
        obj.insert("label".into(), Value::String(group_name.clone()));
        obj.insert("order".into(), json!(gi as u32));
        obj.insert("size_variant".into(), Value::Null);
        obj.insert("type".into(), Value::String("group".into()));
        obj.insert("visual_notes".into(), Value::Null);
        groups.push(Value::Object(obj));
    }
    Ok(groups)
}

fn control_to_value(c: &Row, order: usize, aliases: Vec<String>) -> Value {
    let mapped = c.mapped_type().unwrap_or_else(|| "control".to_owned());
    let parent = if c.parent.is_empty() {
        Value::Null
    } else {
        Value::String(c.parent.clone())
    };
    let aliases_json: Vec<Value> = aliases.into_iter().map(Value::String).collect();
    let mut obj = Map::new();
    obj.insert("aliases".into(), Value::Array(aliases_json));
    obj.insert("bounds".into(), Value::Null);
    obj.insert("command".into(), Value::Null);
    obj.insert("icon_ref".into(), Value::Null);
    obj.insert("id".into(), Value::String(c.control_name.clone()));
    obj.insert("keytip".into(), Value::Null);
    obj.insert("label".into(), Value::String(c.control_name.clone()));
    obj.insert("order".into(), json!(order as u32));
    obj.insert("parent".into(), parent);
    obj.insert("size_variant".into(), Value::Null);
    obj.insert("type".into(), Value::String(mapped));
    obj.insert("visual_notes".into(), Value::Null);
    Value::Object(obj)
}

fn build_backstage(rows: &[&Row]) -> ExtractResult<Value> {
    let mut tab_rows: Vec<&Row> = rows
        .iter()
        .copied()
        .filter(|r| r.control_type == "tab" && r.tab.is_empty())
        .collect();
    tab_rows.sort_by_key(|r| r.sheet_row);

    let mut tabs = Vec::new();
    for (i, tab_row) in tab_rows.iter().enumerate() {
        let tab_name = &tab_row.control_name;
        let mut control_rows: Vec<&Row> = rows
            .iter()
            .copied()
            .filter(|r| r.tab == *tab_name && r.control_type != "tab")
            .collect();
        control_rows.sort_by(|a, b| {
            a.policy_id
                .cmp(&b.policy_id)
                .then_with(|| a.control_name.cmp(&b.control_name))
                .then_with(|| a.sheet_row.cmp(&b.sheet_row))
        });

        // For duplicates of the same control name within this
        // backstage tab, the other parents become aliases. The
        // empty string is never a valid parent id, so we filter it
        // out.
        let mut seen: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for c in &control_rows {
            if !c.parent.is_empty() {
                seen.entry(c.control_name.clone())
                    .or_default()
                    .insert(c.parent.clone());
            }
        }
        let mut controls = Vec::new();
        for (ci, c) in control_rows.iter().enumerate() {
            let mut other_parents: Vec<String> = seen
                .get(&c.control_name)
                .map(|s| s.iter().cloned().collect())
                .unwrap_or_default();
            other_parents.retain(|p| p != &c.parent);
            other_parents.sort();
            other_parents.dedup();
            controls.push(control_to_value(c, ci, other_parents));
        }

        let mut obj = Map::new();
        obj.insert("bounds".into(), Value::Null);
        obj.insert("controls".into(), Value::Array(controls));
        obj.insert("icon_ref".into(), Value::Null);
        obj.insert("id".into(), Value::String(tab_name.clone()));
        obj.insert("keytip".into(), Value::Null);
        obj.insert("label".into(), Value::String(tab_name.clone()));
        obj.insert("order".into(), json!(i as u32));
        obj.insert("size_variant".into(), Value::Null);
        obj.insert("type".into(), Value::String("tab".into()));
        obj.insert("visual_notes".into(), Value::Null);
        tabs.push(Value::Object(obj));
    }

    let mut outer = Map::new();
    outer.insert("kind".into(), Value::String("backstage".into()));
    outer.insert("tabs".into(), Value::Array(tabs));
    Ok(Value::Object(outer))
}

fn build_context_menus(rows: &[&Row]) -> ExtractResult<Vec<Value>> {
    let mut menus = BTreeMap::<String, &Row>::new();
    for r in rows {
        if r.control_type == "contextMenu" {
            menus.insert(r.control_name.clone(), r);
        }
    }
    let mut out = Vec::new();
    for (i, (name, row)) in menus.iter().enumerate() {
        let mut obj = Map::new();
        obj.insert("bounds".into(), Value::Null);
        obj.insert("controls".into(), Value::Array(Vec::new()));
        obj.insert("icon_ref".into(), Value::Null);
        obj.insert("id".into(), Value::String(name.clone()));
        obj.insert("keytip".into(), Value::Null);
        obj.insert("label".into(), Value::String(name.clone()));
        obj.insert("order".into(), json!(i as u32));
        obj.insert("size_variant".into(), Value::Null);
        obj.insert("type".into(), Value::String("contextMenu".into()));
        obj.insert("visual_notes".into(), Value::Null);
        let _ = row;
        out.push(Value::Object(obj));
    }
    Ok(out)
}

fn build_qat(rows: &[&Row]) -> ExtractResult<Value> {
    let mut sorted: Vec<&Row> = rows.to_vec();
    sorted.sort_by(|a, b| {
        a.policy_id
            .cmp(&b.policy_id)
            .then_with(|| a.control_name.cmp(&b.control_name))
            .then_with(|| a.sheet_row.cmp(&b.sheet_row))
    });
    let mut seen: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for c in &sorted {
        if !c.parent.is_empty() {
            seen.entry(c.control_name.clone())
                .or_default()
                .insert(c.parent.clone());
        }
    }
    let mut controls = Vec::new();
    for (i, c) in sorted.iter().enumerate() {
        let mut other_parents: Vec<String> = seen
            .get(&c.control_name)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default();
        other_parents.retain(|p| p != &c.parent);
        other_parents.sort();
        other_parents.dedup();
        controls.push(control_to_value(c, i, other_parents));
    }
    let mut outer = Map::new();
    outer.insert("controls".into(), Value::Array(controls));
    outer.insert("id".into(), Value::String("qat".into()));
    Ok(Value::Object(outer))
}

// ---- Tests -----------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use calamine::Cell;

    fn cell<S: Into<String>>(s: S) -> Data {
        Data::String(s.into())
    }

    /// Build a `Range<Data>` from a list of rows. Each row is a
    /// tuple of (row_index, list of (col, value)). The values are
    /// `Data`. Empty cells are filled with `Data::Empty`.
    fn make_range(cells: Vec<(u32, Vec<(u32, Data)>)>) -> Range<Data> {
        let mut flat: Vec<Cell<Data>> = Vec::new();
        let mut max_row = 0u32;
        let mut max_col = 0u32;
        for (r, row) in &cells {
            if *r > max_row {
                max_row = *r;
            }
            for (c, v) in row {
                if *c > max_col {
                    max_col = *c;
                }
                flat.push(Cell::new((*r, *c), v.clone()));
            }
        }
        if flat.is_empty() {
            return Range::empty();
        }
        flat.sort_by_key(|c| (c.get_position().0, c.get_position().1));
        let r = Range::from_sparse(flat);
        // from_sparse may compute a smaller end if the last cell
        // wasn't at the max row/col. For our tests it doesn't matter.
        let _ = (max_row, max_col);
        r
    }

    fn make_range_from_rows(row_values: &[Vec<Data>]) -> Range<Data> {
        let mut cells: Vec<(u32, Vec<(u32, Data)>)> = Vec::new();
        for (i, row) in row_values.iter().enumerate() {
            let row_cells: Vec<(u32, Data)> = row
                .iter()
                .enumerate()
                .map(|(j, v)| (j as u32, v.clone()))
                .collect();
            cells.push((i as u32, row_cells));
        }
        make_range(cells)
    }

    fn hdr() -> Vec<Data> {
        vec![
            cell("Control Name"),
            cell("Control Type"),
            cell("Tab Set"),
            cell("Tab"),
            cell("Group/Context Menu Name"),
            cell("Parent Control"),
            cell("Policy ID"),
        ]
    }

    fn row_vec(cells: Vec<&str>) -> Vec<Data> {
        cells.into_iter().map(cell).collect()
    }

    fn rows_from_strings(rows: &[Vec<&str>]) -> Range<Data> {
        let mut all: Vec<Vec<Data>> = Vec::new();
        all.push(hdr());
        for r in rows {
            all.push(row_vec(r.clone()));
        }
        make_range_from_rows(&all)
    }

    #[test]
    fn parse_header_and_one_row() {
        let range = rows_from_strings(&[vec![
            "FileSave",
            "button",
            "None (Quick Access Toolbar)",
            "Quick Access Toolbar",
            "",
            "",
            "3",
        ]]);
        let rows = rows_from_range(&range, Path::new("test.xlsx")).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].control_name, "FileSave");
        assert_eq!(rows[0].control_type, "button");
        assert_eq!(rows[0].policy_id, 3);
        assert!(matches!(rows[0].section(), Section::Qat));
    }

    #[test]
    fn section_routing() {
        let cases: &[(&str, &str, Section)] = &[
            ("None (Core Tab)", "", Section::CommandBar),
            (
                "TabSetPictureTools",
                "TabPictureToolsFormat",
                Section::CommandBar,
            ),
            ("None (Backstage View)", "TabInfo", Section::Backstage),
            (
                "None (Context Menu)",
                "None (Context Menu)",
                Section::ContextMenu,
            ),
            (
                "None (Quick Access Toolbar)",
                "Quick Access Toolbar",
                Section::Qat,
            ),
            (
                "None (Not in the Ribbon)",
                "None (Not in the Ribbon)",
                Section::Drop,
            ),
            ("", "", Section::Drop),
        ];
        for (ts, tb, expected) in cases {
            let r = Row {
                sheet_row: 1,
                control_name: "X".into(),
                control_type: "button".into(),
                tab_set: (*ts).into(),
                tab: (*tb).into(),
                group: String::new(),
                parent: String::new(),
                policy_id: 1,
            };
            assert_eq!(r.section(), *expected, "ts={ts} tb={tb}");
        }
    }

    #[test]
    fn mapped_type_normalisation() {
        let cases: &[(&str, &str)] = &[
            ("button", "button"),
            ("button (dialogBoxLauncher)", "buttonDialogBoxLauncher"),
            ("None (Legacy)", "none"),
            ("toggleButton", "toggleButton"),
        ];
        for (raw, expected) in cases {
            let r = Row {
                sheet_row: 1,
                control_name: "X".into(),
                control_type: (*raw).into(),
                tab_set: String::new(),
                tab: String::new(),
                group: String::new(),
                parent: String::new(),
                policy_id: 1,
            };
            assert_eq!(r.mapped_type().as_deref(), Some(*expected), "raw={raw}");
        }
    }

    /// R3: the unknown-control-type error names the workbook, not an
    /// `<in-memory>` placeholder.
    #[test]
    fn unknown_control_type_error_names_the_source_file() {
        let range = rows_from_strings(&[vec!["X", "explodingButton", "", "", "", "", "1"]]);
        let rows = rows_from_range(&range, Path::new("/data/panel-2016.xlsx")).unwrap();
        let refs = collect(&rows);
        let err = assemble_layout(
            &refs,
            Path::new("/data/panel-2016.xlsx"),
            "s",
            "https://e/",
            "r",
        )
        .unwrap_err();
        assert!(err.to_string().contains("/data/panel-2016.xlsx"), "{err}");
    }

    #[test]
    fn mapped_type_rejects_unknown() {
        let r = Row {
            sheet_row: 1,
            control_name: "X".into(),
            control_type: "explodingButton".into(),
            tab_set: String::new(),
            tab: String::new(),
            group: String::new(),
            parent: String::new(),
            policy_id: 1,
        };
        assert_eq!(r.mapped_type(), None);
    }

    fn collect(rows: &[Row]) -> Vec<&Row> {
        rows.iter().collect()
    }

    #[test]
    fn deterministic_yaml_for_same_input() {
        let range = rows_from_strings(&[
            vec!["TabHome", "tab", "None (Core Tab)", "", "", "", "100"],
            vec![
                "GroupClipboard",
                "group",
                "None (Core Tab)",
                "TabHome",
                "",
                "",
                "101",
            ],
            vec![
                "Paste",
                "button",
                "None (Core Tab)",
                "TabHome",
                "GroupClipboard",
                "PasteMenu",
                "22",
            ],
            vec![
                "Cut",
                "button",
                "None (Core Tab)",
                "TabHome",
                "GroupClipboard",
                "",
                "21",
            ],
            vec![
                "PasteMenu",
                "splitButton",
                "None (Core Tab)",
                "TabHome",
                "GroupClipboard",
                "",
                "102",
            ],
        ]);
        let rows = rows_from_range(&range, Path::new("test.xlsx")).unwrap();
        let refs = collect(&rows);
        let a = assemble_layout(
            &refs,
            Path::new("test.xlsx"),
            "test",
            "https://example.invalid/",
            "rev",
        )
        .unwrap();
        let b = assemble_layout(
            &refs,
            Path::new("test.xlsx"),
            "test",
            "https://example.invalid/",
            "rev",
        )
        .unwrap();
        let ya = serde_saphyr::to_string(&a).unwrap();
        let yb = serde_saphyr::to_string(&b).unwrap();
        assert_eq!(ya, yb);
        // The first tab is TabHome, and its id is the Fluent name.
        let tabs = a.get("tabs").unwrap().as_array().unwrap();
        assert_eq!(tabs[0].get("id").unwrap().as_str(), Some("TabHome"));
    }

    #[test]
    fn layout_uses_fluent_id_not_command_id() {
        let range =
            rows_from_strings(&[vec!["TabHome", "tab", "None (Core Tab)", "", "", "", "1"]]);
        let rows = rows_from_range(&range, Path::new("test.xlsx")).unwrap();
        let refs = collect(&rows);
        let layout = assemble_layout(
            &refs,
            Path::new("test.xlsx"),
            "src",
            "https://example/",
            "rev",
        )
        .unwrap();
        let tabs = layout.get("tabs").unwrap().as_array().unwrap();
        let first = &tabs[0];
        let id = first.get("id").unwrap();
        assert_eq!(id, "TabHome");
        assert_ne!(id, "home");
    }

    /// R4: an empty Policy ID cell means "unordered" (sorts last);
    /// a non-empty unparsable cell is a Parse error naming the row.
    #[test]
    fn policy_id_parsing() {
        let all: Vec<Vec<Data>> = vec![
            hdr(),
            vec![
                cell("FileSave"),
                cell("button"),
                cell("None (Quick Access Toolbar)"),
                cell("Quick Access Toolbar"),
                cell(""),
                cell(""),
                cell(""),
            ],
        ];
        let range = make_range_from_rows(&all);
        let rows = rows_from_range(&range, Path::new("test.xlsx")).unwrap();
        assert_eq!(rows[0].policy_id, NO_POLICY_ID, "empty cell sorts last");

        let all: Vec<Vec<Data>> = vec![
            hdr(),
            vec![
                cell("FileSave"),
                cell("button"),
                cell("None (Quick Access Toolbar)"),
                cell("Quick Access Toolbar"),
                cell(""),
                cell(""),
                cell("not-a-number"),
            ],
        ];
        let range = make_range_from_rows(&all);
        let err = rows_from_range(&range, Path::new("test.xlsx")).unwrap_err();
        assert!(matches!(err, ExtractError::Parse { .. }), "{err}");
        let msg = err.to_string();
        assert!(msg.contains("row 2"), "{msg}");
        assert!(msg.contains("Policy ID"), "{msg}");
        assert!(msg.contains("test.xlsx"), "{msg}");
    }

    #[test]
    fn numeric_policy_id_in_cell() {
        let all: Vec<Vec<Data>> = vec![
            hdr(),
            vec![
                cell("FileSave"),
                cell("button"),
                cell("None (Quick Access Toolbar)"),
                cell("Quick Access Toolbar"),
                cell(""),
                cell(""),
                Data::Int(3),
            ],
        ];
        let range = make_range_from_rows(&all);
        let rows = rows_from_range(&range, Path::new("test.xlsx")).unwrap();
        assert_eq!(rows[0].policy_id, 3);
    }

    #[test]
    fn fixture_validates_against_schema() {
        let range = rows_from_strings(&[
            vec!["TabHome", "tab", "None (Core Tab)", "", "", "", "1"],
            vec![
                "GroupClipboard",
                "group",
                "None (Core Tab)",
                "TabHome",
                "",
                "",
                "2",
            ],
            vec![
                "FileSave",
                "button",
                "None (Core Tab)",
                "TabHome",
                "GroupClipboard",
                "",
                "3",
            ],
            vec!["TabInfo", "tab", "None (Backstage View)", "", "", "", "4"],
            vec![
                "FileSaveBackstage",
                "button",
                "None (Backstage View)",
                "TabInfo",
                "",
                "",
                "5",
            ],
            vec![
                "FileSave",
                "button",
                "None (Quick Access Toolbar)",
                "Quick Access Toolbar",
                "",
                "",
                "6",
            ],
            vec![
                "ContextMenuShape",
                "contextMenu",
                "None (Context Menu)",
                "None (Context Menu)",
                "",
                "",
                "7",
            ],
        ]);
        let rows = rows_from_range(&range, Path::new("test.xlsx")).unwrap();
        let refs = collect(&rows);
        let layout = assemble_layout(
            &refs,
            Path::new("test.xlsx"),
            "test",
            "https://example.invalid/",
            "rev",
        )
        .unwrap();
        let text = serde_saphyr::to_string(&layout).unwrap();

        // Sanity: tabs[0].id is TabHome (Fluent name).
        let parsed: Value = serde_saphyr::from_str(&text).unwrap();
        assert_eq!(
            parsed.get("tabs").unwrap().as_array().unwrap()[0]
                .get("id")
                .unwrap()
                .as_str(),
            Some("TabHome")
        );

        // Validate against the committed schema (skip if not present
        // in the build environment).
        let schema_path = crate::util::root().join("ui-spec").join("schema.json");
        if schema_path.is_file() {
            let schema = Schema::load(&schema_path).unwrap();
            schema
                .validate_str(
                    &text,
                    crate::extract::schema::SchemaKind::Layout,
                    Path::new("ui-spec/panel-2016.yaml"),
                )
                .unwrap();
        }
    }

    #[test]
    fn real_xlsx_round_trip_is_byte_stable() {
        let path = crate::util::root()
            .join("reference")
            .join("fluent-ui-ids")
            .join("Office 2016")
            .join("visiocontrols.xlsx");
        if !path.is_file() {
            eprintln!("reference xlsx missing; skipping round-trip test");
            return;
        }
        let a = build_layout(
            &path,
            "Office Fluent UI 2016, Visio",
            "https://example/",
            "rev",
        )
        .unwrap();
        let b = build_layout(
            &path,
            "Office Fluent UI 2016, Visio",
            "https://example/",
            "rev",
        )
        .unwrap();
        assert_eq!(a.text, b.text);
        assert!(a.text.contains("TabHome"));
    }

    #[test]
    fn parent_field_set_for_nested_control() {
        let range = rows_from_strings(&[
            vec!["TabHome", "tab", "None (Core Tab)", "", "", "", "1"],
            vec![
                "GroupClipboard",
                "group",
                "None (Core Tab)",
                "TabHome",
                "",
                "",
                "2",
            ],
            vec![
                "PasteMenu",
                "splitButton",
                "None (Core Tab)",
                "TabHome",
                "GroupClipboard",
                "",
                "3",
            ],
            vec![
                "Paste",
                "button",
                "None (Core Tab)",
                "TabHome",
                "GroupClipboard",
                "PasteMenu",
                "4",
            ],
        ]);
        let rows = rows_from_range(&range, Path::new("test.xlsx")).unwrap();
        let refs = collect(&rows);
        let layout =
            assemble_layout(&refs, Path::new("test.xlsx"), "s", "https://e/", "r").unwrap();
        let controls = layout.get("tabs").unwrap().as_array().unwrap()[0]
            .get("groups")
            .unwrap()
            .as_array()
            .unwrap()[0]
            .get("controls")
            .unwrap()
            .as_array()
            .unwrap();
        let paste = controls
            .iter()
            .find(|c| c.get("id").and_then(|v| v.as_str()) == Some("Paste"))
            .expect("Paste exists");
        assert_eq!(paste.get("parent").unwrap().as_str(), Some("PasteMenu"));
    }

    #[test]
    fn cross_tab_duplicates_become_aliases() {
        let range = rows_from_strings(&[
            vec!["TabHome", "tab", "None (Core Tab)", "", "", "", "1"],
            vec![
                "GroupClipboard",
                "group",
                "None (Core Tab)",
                "TabHome",
                "",
                "",
                "2",
            ],
            vec![
                "Copy",
                "button",
                "None (Core Tab)",
                "TabHome",
                "GroupClipboard",
                "",
                "3",
            ],
            vec!["TabIconEditor", "tab", "None (Core Tab)", "", "", "", "4"],
            vec![
                "GroupIconEditClipboard",
                "group",
                "None (Core Tab)",
                "TabIconEditor",
                "",
                "",
                "5",
            ],
            vec![
                "Copy",
                "button",
                "None (Core Tab)",
                "TabIconEditor",
                "GroupIconEditClipboard",
                "",
                "6",
            ],
        ]);
        let rows = rows_from_range(&range, Path::new("test.xlsx")).unwrap();
        let refs = collect(&rows);
        let layout =
            assemble_layout(&refs, Path::new("test.xlsx"), "s", "https://e/", "r").unwrap();
        let tabs = layout.get("tabs").unwrap().as_array().unwrap();
        let home = tabs
            .iter()
            .find(|t| t.get("id").and_then(|v| v.as_str()) == Some("TabHome"))
            .unwrap();
        let clip = home.get("groups").unwrap().as_array().unwrap()[0]
            .get("controls")
            .unwrap()
            .as_array()
            .unwrap();
        let copy = clip
            .iter()
            .find(|c| c.get("id").and_then(|v| v.as_str()) == Some("Copy"))
            .expect("Copy in TabHome");
        let aliases = copy.get("aliases").unwrap().as_array().unwrap();
        let alias_strs: Vec<&str> = aliases.iter().filter_map(|v| v.as_str()).collect();
        assert!(
            alias_strs.contains(&"GroupIconEditClipboard"),
            "aliases: {alias_strs:?}"
        );
    }

    /// M2 of the P1-03 review: prove the committed layouts load
    /// with the runtime [`larrygram_ui_spec::Loader`]. Mirrors the
    /// loader's data set in a temp workspace — `ui-spec/` whole (the
    /// v1 pair, the shared tables and the v2 `layouts/` and `themes/`
    /// documents, task P7-03) plus the icon set its `icon_file`
    /// cross-check resolves — and calls `Loader::load` for both v1
    /// layouts.
    #[test]
    fn committed_layouts_load_with_the_runtime_loader() {
        use larrygram_ui_spec::{LayoutKind, Loader};

        let ws = crate::util::root();
        let committed_2016 = ws.join("ui-spec").join("panel-2016.yaml");
        let committed_365 = ws.join("ui-spec").join("panel-365.yaml");
        if !committed_2016.is_file() || !committed_365.is_file() {
            eprintln!("committed layouts missing; skipping loader test");
            return;
        }

        let tmp = crate::util::test_support::TempDir::new("fluent-loader");
        crate::util::test_support::copy_tree(&ws.join("ui-spec"), &tmp.join("ui-spec"));
        crate::util::test_support::copy_tree(
            &ws.join("crates/ui/assets/icons/tabler"),
            &tmp.join("crates/ui/assets/icons/tabler"),
        );

        let loader = Loader::new(tmp.path()).expect("loader compiles");
        let set_2016 = loader.load(LayoutKind::Panel2016).expect("2016 loads");
        let set_365 = loader.load(LayoutKind::Panel365).expect("365 loads");

        // Counts match the values reported in the Result section.
        // 2016: 16 command-bar tabs + 8 backstage + 43 context menus + 9 QAT.
        assert_eq!(set_2016.layout.tabs.len(), 16, "2016 command bar tabs");
        assert_eq!(
            set_2016.layout.backstage.tabs.len(),
            8,
            "2016 backstage tabs"
        );
        assert_eq!(
            set_2016.layout.context_menus.len(),
            43,
            "2016 context menus"
        );
        assert_eq!(
            set_2016.layout.quick_access_toolbar.controls.len(),
            9,
            "2016 QAT controls"
        );

        // 365: 19 command-bar tabs + 10 backstage + 43 context menus + 9 QAT.
        assert_eq!(set_365.layout.tabs.len(), 19, "365 command bar tabs");
        assert_eq!(
            set_365.layout.backstage.tabs.len(),
            10,
            "365 backstage tabs"
        );
        assert_eq!(set_365.layout.context_menus.len(), 43, "365 context menus");
        assert_eq!(
            set_365.layout.quick_access_toolbar.controls.len(),
            9,
            "365 QAT controls"
        );
    }
}
