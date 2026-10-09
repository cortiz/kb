//! A variable value that knows whether it is a secret. `Debug` never prints a secret, so a value
//! held in UI state or printed in a panic message cannot leak it (FR-013).

use std::fmt;

#[derive(Clone, PartialEq, Eq)]
pub struct Value {
    text: String,
    secret: bool,
}

impl Value {
    pub fn plain(text: impl Into<String>) -> Value {
        Value {
            text: text.into(),
            secret: false,
        }
    }

    pub fn secret(text: impl Into<String>) -> Value {
        Value {
            text: text.into(),
            secret: true,
        }
    }

    /// A value of the same kind as `secret` says.
    pub fn new(text: impl Into<String>, secret: bool) -> Value {
        Value {
            text: text.into(),
            secret,
        }
    }

    pub fn is_secret(&self) -> bool {
        self.secret
    }

    /// The text itself. Call this only where the value is meant to be used (substitution, the
    /// field editor); never to build a message.
    pub fn reveal(&self) -> &str {
        &self.text
    }
}

/// What a secret shows as, in previews and `Debug`.
pub const MASK: &str = "••••";

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.secret {
            write!(f, "Value({MASK})")
        } else {
            write!(f, "Value({:?})", self.text)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_never_debug_prints() {
        let s = Value::secret("S3CR3T-MARKER");
        assert!(!format!("{s:?}").contains("S3CR3T"));
        assert!(format!("{s:?}").contains(MASK));
        let c = s.clone();
        assert!(!format!("{c:?}").contains("S3CR3T"));
        assert!(!format!("{:#?}", vec![c]).contains("S3CR3T"));
        assert_eq!(s.reveal(), "S3CR3T-MARKER");
    }

    #[test]
    fn a_plain_value_prints() {
        assert_eq!(format!("{:?}", Value::plain("prod")), "Value(\"prod\")");
    }
}
