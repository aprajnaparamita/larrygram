//! Run-time list providers (ADR 0061; `docs/ui/layout-format.md`
//! "Run-time lists").
//!
//! A layout item with a `list` name is filled at run time from live
//! data: File > recent files, Window > open windows, Edit > Go To >
//! pages and the rest. [`ListProvider`] is the seam: [`LiveLists`
//! answers the three lists that come from the application state today
//! and returns an empty list for every other name, so a caller that
//! needs more fills it in with its own provider (task P7-07 adds its
//! lists here). An empty list renders one disabled "(none)" row in the
//! menu widgets.
//!
//! An entry may carry a command ([`ListEntry::command`]); activating it
//! dispatches that command with [`ListEntry::args`] (ADR 0028). The
//! window and page lists have no switching command yet, so their
//! entries carry the target name as an argument and render disabled
//! until the commands exist (see the P7-06 Result, Open issues).

use larrygram_commands::app::AppCtx;
use larrygram_commands::{ArgValue, Args, CommandId};

/// One entry of a run-time list.
#[derive(Debug, Clone, PartialEq)]
pub struct ListEntry {
    /// The row label ("1. drawing.vsdx", "Page-1").
    pub label: String,
    /// Registry id dispatched on activation; `None` renders the row
    /// disabled (the list has no command for it yet).
    pub command: Option<CommandId>,
    /// Arguments merged into the activation's [`Invocation`].
    pub args: Args,
    /// The entry is the active one (Window > the active window).
    pub checked: bool,
    /// Access-key letter shown underlined in menus.
    pub access: Option<char>,
    /// Key into `ui-spec/icons.yaml`, when the entry has an icon.
    pub icon: Option<String>,
}

impl ListEntry {
    /// An entry with a label and no command.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            command: None,
            args: Args::new(),
            checked: false,
            access: None,
            icon: None,
        }
    }

    /// The entry with a command and its arguments.
    pub fn with_command(mut self, command: CommandId, args: Args) -> Self {
        self.command = Some(command);
        self.args = args;
        self
    }

    /// The entry with an access-key letter.
    pub fn with_access(mut self, access: char) -> Self {
        self.access = Some(access);
        self
    }

    /// The entry marked as the active one.
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }
}

/// Fills the run-time lists of the Panel Classic and Panel Modern
/// widgets.
///
/// The default implementation answers every list with no entries; the
/// widgets then draw one disabled "(none)" row. Providers wrap other
/// providers to add or override single lists.
pub trait ListProvider {
    /// The entries of the run-time list `name` (`recent_files`,
    /// `windows`, `pages`, ...), in the order they are drawn.
    fn list(&self, name: &str, app: &AppCtx) -> Vec<ListEntry> {
        let _ = (name, app);
        Vec::new()
    }
}

/// The run-time lists that come from the application state (task
/// P7-06): `recent_files`, `windows` and `pages`.
///
/// Every other name answers empty (the provider trait's default).
/// Window and page entries carry the target name in their `args` but
/// no command yet: the registry has no window- or page-switching id
/// (Open issue in the P7-06 Result), so the rows render disabled.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LiveLists;

impl ListProvider for LiveLists {
    fn list(&self, name: &str, app: &AppCtx) -> Vec<ListEntry> {
        match name {
            "recent_files" => recent_files(app),
            "windows" => windows(app),
            "pages" => pages(app),
            _ => Vec::new(),
        }
    }
}

/// File > recent files: numbered 1-9 with the digit as the access key
/// (`docs/ui/panel-classic.md`), each opening the file.
fn recent_files(app: &AppCtx) -> Vec<ListEntry> {
    let Some(open) = CommandId::parse("file.open").ok() else {
        return Vec::new();
    };
    app.settings
        .recent_files
        .iter()
        .take(9)
        .enumerate()
        .map(|(index, path)| {
            let digit = char::from(b'1' + index as u8);
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string());
            ListEntry::new(format!("{digit}. {name}"))
                .with_command(open, Args::new().with("path", ArgValue::Path(path.clone())))
                .with_access(digit)
        })
        .collect()
}

/// Window > open windows: one row per document session, with a check
/// on the active one.
fn windows(app: &AppCtx) -> Vec<ListEntry> {
    app.documents
        .iter()
        .enumerate()
        .map(|(index, session)| {
            let name = session.display_name();
            ListEntry::new(name.clone())
                .checked(Some(index) == app.active)
                .with_command_args_target("window", &name)
        })
        .collect()
}

/// Edit > Go To > pages: the pages of the active document.
fn pages(app: &AppCtx) -> Vec<ListEntry> {
    let Some(session) = app.active_document() else {
        return Vec::new();
    };
    session
        .document
        .pages
        .iter()
        .map(|page| {
            let name = page.name.clone();
            ListEntry::new(name.clone()).with_command_args_target("page", &name)
        })
        .collect()
}

impl ListEntry {
    /// The entry with a target name but no command yet (the window
    /// and page lists): the argument rides along until the switching
    /// commands exist.
    fn with_command_args_target(mut self, kind: &str, name: &str) -> Self {
        self.args.set(kind, ArgValue::Text(name.to_owned()));
        self
    }
}

#[cfg(test)]
mod tests {
    use larrygram_commands::app::Settings;

    use super::*;

    /// An app with one document and one page.
    fn app() -> AppCtx {
        let mut app = AppCtx::new(Settings::default());
        let mut document = larrygram_model::Document::default();
        let mut page = larrygram_model::Page::new(larrygram_model::PageId { index: 0 }, 1);
        page.name = "Page-1".to_owned();
        document.pages.push(page);
        app.open_document(larrygram_commands::DocumentSession::new(document, None));
        app
    }

    #[test]
    fn an_unknown_list_is_empty_by_default() {
        let provider = LiveLists;
        assert!(provider.list("themes", &app()).is_empty());
        assert!(ListProvider::list(&NoProvider, "recent_files", &app()).is_empty());
    }

    /// A provider that answers nothing (the trait default).
    struct NoProvider;
    impl ListProvider for NoProvider {}

    #[test]
    fn recent_files_are_numbered_with_the_digit_as_the_access_key() {
        let mut app = app();
        app.settings.recent_files = vec![
            std::path::PathBuf::from("/tmp/first.vsdx"),
            std::path::PathBuf::from("/tmp/second.vsdx"),
        ];
        let entries = LiveLists.list("recent_files", &app);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].label, "1. first.vsdx");
        assert_eq!(entries[0].access, Some('1'));
        assert_eq!(entries[1].label, "2. second.vsdx");
        assert_eq!(entries[1].access, Some('2'));
        let command = entries[0].command.as_ref().expect("file.open");
        assert_eq!(command.as_str(), "file.open");
        assert_eq!(
            entries[0].args.path("path"),
            Some(std::path::Path::new("/tmp/first.vsdx"))
        );
    }

    #[test]
    fn recent_files_stop_at_nine() {
        let mut app = app();
        app.settings.recent_files = (0..12)
            .map(|n| std::path::PathBuf::from(format!("/tmp/{n}.vsdx")))
            .collect();
        let entries = LiveLists.list("recent_files", &app);
        assert_eq!(entries.len(), 9);
        assert_eq!(entries[8].access, Some('9'));
    }

    #[test]
    fn windows_check_the_active_document_and_carry_the_target_name() {
        let entries = LiveLists.list("windows", &app());
        assert_eq!(entries.len(), 1);
        assert!(entries[0].checked, "the active window is checked");
        assert_eq!(
            entries[0].args.text("window"),
            Some(entries[0].label.as_str())
        );
        assert!(
            entries[0].command.is_none(),
            "no window-switching command exists yet"
        );
    }

    #[test]
    fn pages_list_the_active_document_pages() {
        let entries = LiveLists.list("pages", &app());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].label, "Page-1");
        assert_eq!(entries[0].args.text("page"), Some("Page-1"));
        assert!(entries[0].command.is_none());
    }

    #[test]
    fn pages_of_a_session_without_a_document_are_empty() {
        let app = AppCtx::new(Settings::default());
        assert!(LiveLists.list("pages", &app).is_empty());
    }

    #[test]
    fn entries_build_through_the_builder() {
        let entry = ListEntry::new("Cut").checked(true).with_access('t');
        assert_eq!(entry.label, "Cut");
        assert!(entry.checked);
        assert_eq!(entry.access, Some('t'));
        assert!(entry.command.is_none());
    }
}
