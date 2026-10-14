//! Shell dialects, quoting, and syntax conventions.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellDialect {
    Zsh,
    Bash,
    Fish,
}

impl ShellDialect {
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "zsh" => Some(ShellDialect::Zsh),
            "bash" => Some(ShellDialect::Bash),
            "fish" => Some(ShellDialect::Fish),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ShellDialect::Zsh => "zsh",
            ShellDialect::Bash => "bash",
            ShellDialect::Fish => "fish",
        }
    }

    /// Formats a reference to an environment variable in this shell's syntax.
    pub fn env_var_ref(&self, var_name: &str) -> String {
        match self {
            ShellDialect::Zsh | ShellDialect::Bash | ShellDialect::Fish => {
                format!("\"${var_name}\"")
            }
        }
    }

    /// Checks if a string contains only characters safe without quoting in POSIX and fish shells.
    fn is_safe_unquoted(s: &str) -> bool {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/'))
    }

    /// Quotes an argument safely for this shell dialect.
    pub fn quote_arg(&self, arg: &str) -> String {
        if Self::is_safe_unquoted(arg) {
            return arg.to_string();
        }
        match self {
            ShellDialect::Zsh | ShellDialect::Bash => {
                format!("'{}'", arg.replace('\'', r"'\''"))
            }
            ShellDialect::Fish => {
                format!("'{}'", arg.replace('\'', r"\'"))
            }
        }
    }

    /// Shell command snippet used to initialize this shell via eval or source.
    pub fn init_eval_command(&self) -> String {
        match self {
            ShellDialect::Zsh => "eval \"$(kb shell init zsh)\"".to_string(),
            ShellDialect::Bash => "eval \"$(kb shell init bash)\"".to_string(),
            ShellDialect::Fish => "kb shell init fish | source".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialect_from_str_and_as_str() {
        assert_eq!(ShellDialect::from_str("zsh"), Some(ShellDialect::Zsh));
        assert_eq!(ShellDialect::from_str("ZSH"), Some(ShellDialect::Zsh));
        assert_eq!(ShellDialect::from_str("bash"), Some(ShellDialect::Bash));
        assert_eq!(ShellDialect::from_str("Bash"), Some(ShellDialect::Bash));
        assert_eq!(ShellDialect::from_str("fish"), Some(ShellDialect::Fish));
        assert_eq!(ShellDialect::from_str("FISH"), Some(ShellDialect::Fish));
        assert_eq!(ShellDialect::from_str("powershell"), None);
        assert_eq!(ShellDialect::from_str("csh"), None);

        assert_eq!(ShellDialect::Zsh.as_str(), "zsh");
        assert_eq!(ShellDialect::Bash.as_str(), "bash");
        assert_eq!(ShellDialect::Fish.as_str(), "fish");
    }

    #[test]
    fn env_var_reference_rules() {
        assert_eq!(
            ShellDialect::Zsh.env_var_ref("SUMO_TOKEN"),
            "\"$SUMO_TOKEN\""
        );
        assert_eq!(
            ShellDialect::Bash.env_var_ref("SUMO_TOKEN"),
            "\"$SUMO_TOKEN\""
        );
        assert_eq!(
            ShellDialect::Fish.env_var_ref("SUMO_TOKEN"),
            "\"$SUMO_TOKEN\""
        );
        assert_eq!(
            ShellDialect::Zsh.env_var_ref("AWS_REGION"),
            "\"$AWS_REGION\""
        );
    }

    #[test]
    fn quote_arg_safe_and_escaped() {
        // Safe characters do not require quotes
        assert_eq!(ShellDialect::Zsh.quote_arg("simple"), "simple");
        assert_eq!(
            ShellDialect::Bash.quote_arg("file_name-1.2.txt"),
            "file_name-1.2.txt"
        );
        assert_eq!(ShellDialect::Fish.quote_arg("path/to/dir"), "path/to/dir");

        // Empty string requires quotes
        assert_eq!(ShellDialect::Zsh.quote_arg(""), "''");
        assert_eq!(ShellDialect::Bash.quote_arg(""), "''");
        assert_eq!(ShellDialect::Fish.quote_arg(""), "''");

        // Spaces require single quotes
        assert_eq!(ShellDialect::Zsh.quote_arg("hello world"), "'hello world'");
        assert_eq!(ShellDialect::Bash.quote_arg("hello world"), "'hello world'");
        assert_eq!(ShellDialect::Fish.quote_arg("hello world"), "'hello world'");

        // Single quotes inside strings are safely escaped
        assert_eq!(ShellDialect::Zsh.quote_arg("don't"), "'don'\\''t'");
        assert_eq!(ShellDialect::Bash.quote_arg("don't"), "'don'\\''t'");
        assert_eq!(ShellDialect::Fish.quote_arg("don't"), "'don\\'t'");
    }

    #[test]
    fn eval_init_command_formatting() {
        assert_eq!(
            ShellDialect::Zsh.init_eval_command(),
            "eval \"$(kb shell init zsh)\""
        );
        assert_eq!(
            ShellDialect::Bash.init_eval_command(),
            "eval \"$(kb shell init bash)\""
        );
        assert_eq!(
            ShellDialect::Fish.init_eval_command(),
            "kb shell init fish | source"
        );
    }
}
