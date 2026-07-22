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

fn is_valid(value: &str) -> bool {
    if value.is_empty()
        || value == "@"
        || value.starts_with('-')
        || value.starts_with('/')
        || value.ends_with('/')
        || value.ends_with('.')
        || value.contains("//")
        || value.contains("..")
        || value.contains("@{")
        || value.chars().any(|character| character.is_control() || character.is_whitespace())
        || value
            .chars()
            .any(|character| matches!(character, '~' | '^' | ':' | '?' | '*' | '[' | '\\'))
    {
        return false;
    }

    value.split('/').all(|component| {
        !component.is_empty() && !component.starts_with('.') && !component.ends_with(".lock")
    })
}

#[cfg(test)]
mod tests {
    use super::BranchName;

    #[test]
    fn accepts_branch_names_with_slashes() {
        assert!(BranchName::new("feature/login").is_ok());
        assert!(BranchName::new("release-1.2").is_ok());
    }

    #[test]
    fn rejects_unsafe_and_invalid_ref_names() {
        for value in [
            "",
            " ",
            "-main",
            ".main",
            "main.",
            "main.lock",
            "feature//login",
            "feature/../main",
            "feature@{1}",
            "feature login",
            "main~1",
            "main^",
            "main:",
            "main?",
            "main*",
            "main[",
            "main\\branch",
            "@",
        ] {
            assert!(BranchName::new(value).is_err(), "accepted {value:?}");
        }
    }
}
