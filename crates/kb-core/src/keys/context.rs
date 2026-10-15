/// A screen the keys are resolved for. The names are the `[tui.keys.<name>]` tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Context {
    /// Both main-list modes; never the sub-views.
    Global,
    Search,
    Normal,
    ActionMenu,
    CodeView,
    History,
    Conflict,
    Form,
    Add,
    Help,
}

impl Context {
    pub const ALL: [Context; 10] = [
        Context::Global,
        Context::Search,
        Context::Normal,
        Context::ActionMenu,
        Context::CodeView,
        Context::History,
        Context::Conflict,
        Context::Form,
        Context::Add,
        Context::Help,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Context::Global => "global",
            Context::Search => "search",
            Context::Normal => "normal",
            Context::ActionMenu => "action-menu",
            Context::CodeView => "code-view",
            Context::History => "history",
            Context::Conflict => "conflict",
            Context::Form => "form",
            Context::Add => "add",
            Context::Help => "help",
        }
    }

    pub fn from_name(s: &str) -> Option<Context> {
        Context::ALL.into_iter().find(|c| c.name() == s)
    }

    /// The main list in one of its two modes.
    pub const fn is_list(self) -> bool {
        matches!(self, Context::Search | Context::Normal)
    }

    /// Typing goes into a text field, so plain printable keys cannot be bound.
    pub const fn is_text(self) -> bool {
        matches!(
            self,
            Context::Global | Context::Search | Context::Form | Context::Add
        )
    }

    /// Two-key sequences are dispatched only in normal mode.
    pub const fn allows_sequences(self) -> bool {
        matches!(self, Context::Normal)
    }
}
