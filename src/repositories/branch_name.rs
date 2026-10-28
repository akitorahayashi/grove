use std::fmt;

use crate::AppError;

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BranchName(String);

impl BranchName {
    pub fn new(value: &str) -> Result<Self, AppError> {
        if is_valid(value) {
            Ok(Self(value.to_string()))
        } else {
            Err(AppError::invalid_arguments(format!("invalid Git branch name '{value}'")))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for BranchName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("BranchName").field(&self.0).finish()
    }
}

impl fmt::Display for BranchName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl AsRef<str> for BranchName {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

/// Git's rules for a literal branch name, as `git check-ref-format --branch`
/// applies them, with `@` also refused: Git accepts `refs/heads/@`, but
/// `git switch` resolves `@` to `HEAD`, so such a branch could never be
/// switched to or restored. Git checks bytes, so only ASCII control
/// characters and the ASCII space are excluded; other Unicode, whitespace
/// included, is a valid name that the terminal layer escapes for display.
fn is_valid(value: &str) -> bool {
    if value.is_empty()
        || value == "HEAD"
        || value == "@"
        || value.starts_with('-')
        || value.starts_with('/')
        || value.ends_with('/')
        || value.ends_with('.')
        || value.contains("//")
        || value.contains("..")
        || value.contains("@{")
        || value.chars().any(|character| {
            character.is_ascii_control()
                || matches!(character, ' ' | '~' | '^' | ':' | '?' | '*' | '[' | '\\')
        })
    {
        return false;
    }

    value.split('/').all(|component| !component.starts_with('.') && !component.ends_with(".lock"))
}

#[cfg(test)]
mod tests {
    use super::BranchName;

    /// Each expectation was checked against `git check-ref-format --branch`;
    /// `@` is the one name Git accepts that grove refuses.
    #[test]
    fn accepts_the_literal_branch_names_git_accepts() {
        for value in [
            "feature/login",
            "release-1.2",
            "head",
            "HEAD/x",
            "x@",
            "@x",
            "a\u{a0}b",
            "main\u{a0}",
            "caf\u{e9}",
            "x\u{85}y",
        ] {
            assert!(BranchName::new(value).is_ok(), "rejected {value:?}");
        }
    }

    #[test]
    fn rejects_the_names_git_rejects_and_the_head_shorthand() {
        for value in [
            "",
            " ",
            "HEAD",
            "@",
            "-main",
            ".main",
            "main.",
            "main.lock",
            "x.lock/y",
            "x/.y",
            "/x",
            "x/",
            "feature//login",
            "feature/../main",
            "feature@{1}",
            "feature login",
            "a\tb",
            "a\u{7f}b",
            "main~1",
            "main^",
            "main:",
            "main?",
            "main*",
            "main[",
            "main\\branch",
        ] {
            assert!(BranchName::new(value).is_err(), "accepted {value:?}");
        }
    }
}
