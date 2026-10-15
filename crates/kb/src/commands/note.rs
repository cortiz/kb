//! `kb note [TITLE]`: write a Markdown note. `kb add --type note` does the same.

use clap::Args;

use crate::author::{AddArgs, SourceFlag};

/// The tag that keeps a note away from AI agents.
pub const PRIVATE_TAG: &str = "private";

#[derive(Args, Debug, Clone)]
pub struct NoteArgs {
    /// The title (otherwise the first `# heading` of the body is used).
    pub title: Option<String>,
    /// A tag (repeatable, or comma-separated).
    #[arg(long = "tag", alias = "tags", value_name = "TAG")]
    pub tags: Vec<String>,
    /// Keep AI agents away from this note (adds the `private` tag).
    #[arg(long)]
    pub private: bool,
    /// The new note's ID, for example `note/role-cache-ttl` (folders are created).
    #[arg(long, value_name = "ID")]
    pub path: Option<String>,
    /// Take the body from the clipboard.
    #[arg(long, conflicts_with_all = ["stdin", "empty"])]
    pub clipboard: bool,
    /// Take the body from stdin (also the default when stdin is piped).
    #[arg(long, conflicts_with_all = ["clipboard", "empty"])]
    pub stdin: bool,
    /// Start with no body.
    #[arg(long, conflicts_with_all = ["clipboard", "stdin"])]
    pub empty: bool,
    /// Never ask: save the note or fail.
    #[arg(short = 'y', long)]
    pub yes: bool,
    /// Print the new note as `kb show --json` does.
    #[arg(long)]
    pub json: bool,
}

/// `--tag` values with `private` added when asked for.
pub fn with_private(mut tags: Vec<String>, private: bool) -> Vec<String> {
    if private
        && !crate::author::split_tags(&tags)
            .iter()
            .any(|t| t == PRIVATE_TAG)
    {
        tags.push(PRIVATE_TAG.to_string());
    }
    tags
}

impl NoteArgs {
    /// What `kb add` is asked to do: the editor on a template, unless the body comes from a pipe
    /// or `--yes` says not to ask.
    pub fn into_add(self, stdin_is_terminal: bool) -> AddArgs {
        let piped = !stdin_is_terminal && !self.clipboard && !self.empty;
        let source = match (self.clipboard, self.stdin || piped, self.empty) {
            (true, _, _) => SourceFlag::Clipboard,
            (_, true, _) => SourceFlag::Stdin,
            (_, _, true) => SourceFlag::Empty,
            _ => SourceFlag::Empty,
        };
        let yes = self.yes || piped;
        AddArgs {
            title: self.title,
            ty: Some("note".to_string()),
            tags: with_private(self.tags, self.private),
            path: self.path,
            source,
            editor: !yes,
            yes,
            json: self.json,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args() -> NoteArgs {
        NoteArgs {
            title: Some("T".into()),
            tags: vec!["a".into()],
            private: false,
            path: None,
            clipboard: false,
            stdin: false,
            empty: false,
            yes: false,
            json: false,
        }
    }

    #[test]
    fn a_terminal_gets_the_editor() {
        let add = args().into_add(true);
        assert!(add.editor && !add.yes);
        assert_eq!(add.ty.as_deref(), Some("note"));
        assert_eq!(add.source, SourceFlag::Empty);
    }

    #[test]
    fn a_pipe_or_yes_saves_without_asking() {
        let add = args().into_add(false);
        assert!(add.yes && !add.editor);
        assert_eq!(add.source, SourceFlag::Stdin);
        let add = NoteArgs {
            yes: true,
            ..args()
        }
        .into_add(true);
        assert!(add.yes && !add.editor);
        assert_eq!(add.source, SourceFlag::Empty);
        let add = NoteArgs {
            clipboard: true,
            ..args()
        }
        .into_add(false);
        assert_eq!(add.source, SourceFlag::Clipboard);
    }

    #[test]
    fn private_adds_the_tag_once() {
        assert_eq!(with_private(vec!["a".into()], true), ["a", "private"]);
        assert_eq!(with_private(vec!["a,private".into()], true), ["a,private"]);
        assert_eq!(with_private(vec!["a".into()], false), ["a"]);
    }
}
