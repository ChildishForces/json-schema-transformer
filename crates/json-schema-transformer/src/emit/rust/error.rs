/// A single validation failure: a JSON Pointer (RFC 6901) to the offending
/// location ("" = the whole value) and a human-readable message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JstIssue {
    pub path: String,
    pub message: String,
}

/// Validation error carrying every failed constraint, not just the first.
/// `Display` renders all issues, "; "-separated, each as `path: message`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JstError {
    pub issues: Vec<JstIssue>,
}

impl JstError {
    pub fn single(path: impl Into<String>, message: impl Into<String>) -> JstError {
        JstError {
            issues: vec![JstIssue {
                path: path.into(),
                message: message.into(),
            }],
        }
    }
}

impl From<String> for JstError {
    fn from(message: String) -> JstError {
        JstError::single("", message)
    }
}

impl std::fmt::Display for JstError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (i, issue) in self.issues.iter().enumerate() {
            if i > 0 {
                write!(f, "; ")?;
            }
            if issue.path.is_empty() {
                write!(f, "{}", issue.message)?;
            } else {
                write!(f, "{}: {}", issue.path, issue.message)?;
            }
        }
        Ok(())
    }
}

impl std::error::Error for JstError {}

/// Escape a key for use as a JSON Pointer token (RFC 6901: `~` → `~0`, `/` → `~1`).
pub fn _jst_ptr(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}
