macro_rules! commands {
    ($($variant:ident => $name:literal,)*) => {
        /// What a key does. The names are stable: they are what `[tui.keys.*]` uses.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum Command { $($variant,)* }

        impl Command {
            pub const ALL: &'static [Command] = &[$(Command::$variant,)*];

            pub const fn name(self) -> &'static str {
                match self { $(Command::$variant => $name,)* }
            }

            pub fn from_name(s: &str) -> Option<Command> {
                match s { $($name => Some(Command::$variant),)* _ => None }
            }
        }
    };
}

commands! {
    Pick => "pick",
    Copy => "copy",
    CopyTemplate => "copy-template",
    Open => "open",
    Run => "run",
    ActionMenu => "action-menu",
    Edit => "edit",
    AddEntry => "add-entry",
    RenameEntry => "rename-entry",
    DeleteEntry => "delete-entry",
    Down => "down",
    Up => "up",
    PageDown => "page-down",
    PageUp => "page-up",
    HalfPageDown => "half-page-down",
    HalfPageUp => "half-page-up",
    First => "first",
    Last => "last",
    TypeNext => "type-next",
    TypePrev => "type-prev",
    ScrollPreviewDown => "scroll-preview-down",
    ScrollPreviewUp => "scroll-preview-up",
    ScrollPreviewPageDown => "scroll-preview-page-down",
    ScrollPreviewPageUp => "scroll-preview-page-up",
    TogglePreview => "toggle-preview",
    Left => "left",
    Right => "right",
    LineStart => "line-start",
    LineEnd => "line-end",
    Backspace => "backspace",
    Delete => "delete",
    DeleteWord => "delete-word",
    ClearLine => "clear-line",
    NormalMode => "normal-mode",
    SearchClear => "search-clear",
    SearchKeep => "search-keep",
    Refresh => "refresh",
    Sync => "sync",
    History => "history",
    Help => "help",
    MarkEntry => "mark-entry",
    NewRunbookFromMarks => "new-runbook-from-marks",
    // Esc: clear the query, close the preview, or exit when there is nothing to clear.
    Cancel => "cancel",
    // Exit at once (Ctrl-C is always bound to it).
    Quit => "quit",
    // sub-views
    OpenWeb => "open-web",
    OpenVscode => "open-vscode",
    OpenIdea => "open-idea",
    CopyPathLine => "copy-path-line",
    CopyPermalink => "copy-permalink",
    Repin => "repin",
    Fetch => "fetch",
    Restore => "restore",
    NextField => "next-field",
    PrevField => "prev-field",
    Save => "save",
    EditBody => "edit-body",
    Close => "close",
}
