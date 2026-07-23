use crate::repositories::BranchName;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeStatus {
    branch: Option<String>,
    clean: bool,
}

impl WorktreeStatus {
    pub(crate) fn new(branch: Option<String>, clean: bool) -> Self {
        Self { branch, clean }
    }

    pub fn branch(&self) -> Option<&str> {
        self.branch.as_deref()
    }

    pub fn is_clean(&self) -> bool {
        self.clean
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ParsedWorktreeStatus {
    head: WorktreeHead,
    clean: bool,
}

impl ParsedWorktreeStatus {
    pub(super) fn head(&self) -> &WorktreeHead {
        &self.head
    }

    pub(super) fn is_clean(&self) -> bool {
        self.clean
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum WorktreeHead {
    Branch(String),
    DetachedMarker,
}

pub(super) fn parse(output: &str) -> Option<ParsedWorktreeStatus> {
    let mut oid = None;
    let mut head = None;
    let mut clean = true;

    for line in output.lines() {
        if let Some(value) = line.strip_prefix("# branch.oid ") {
            if oid.is_some()
                || (value != "(initial)"
                    && (value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_hexdigit())))
            {
                return None;
            }
            oid = Some(value);
        } else if let Some(value) = line.strip_prefix("# branch.head ") {
            if head.is_some() || value.is_empty() {
                return None;
            }
            head = Some(if value == "(detached)" {
                WorktreeHead::DetachedMarker
            } else {
                BranchName::new(value).ok()?;
                WorktreeHead::Branch(value.to_string())
            });
        } else if line.starts_with("# ") {
            continue;
        } else if line.is_empty() {
            return None;
        } else {
            clean = false;
        }
    }

    oid?;
    Some(ParsedWorktreeStatus { head: head?, clean })
}

#[cfg(test)]
mod tests {
    use super::{WorktreeHead, parse};

    #[test]
    fn parses_clean_dirty_unborn_and_detached_statuses() {
        let clean =
            parse("# branch.oid abc123\n# branch.head main\n# branch.upstream origin/main\n")
                .unwrap();
        assert_eq!(clean.head(), &WorktreeHead::Branch("main".to_string()));
        assert!(clean.is_clean());

        let dirty =
            parse("# branch.oid abc123\n# branch.head feature/topic\n? untracked.txt\n").unwrap();
        assert_eq!(dirty.head(), &WorktreeHead::Branch("feature/topic".to_string()));
        assert!(!dirty.is_clean());

        let unborn = parse("# branch.oid (initial)\n# branch.head main\n").unwrap();
        assert_eq!(unborn.head(), &WorktreeHead::Branch("main".to_string()));

        let detached = parse("# branch.oid abc123\n# branch.head (detached)\n").unwrap();
        assert_eq!(detached.head(), &WorktreeHead::DetachedMarker);
    }

    #[test]
    fn rejects_missing_duplicate_and_malformed_required_headers() {
        for output in [
            "",
            "# branch.oid abc123\n",
            "# branch.head main\n",
            "# branch.oid not-hex\n# branch.head main\n",
            "# branch.oid abc123\n# branch.oid def456\n# branch.head main\n",
            "# branch.oid abc123\n# branch.head main\n# branch.head other\n",
            "# branch.oid abc123\n# branch.head bad branch\n",
            "# branch.oid abc123\n# branch.head main\n\n? file\n",
        ] {
            assert!(parse(output).is_none(), "accepted malformed output: {output:?}");
        }
    }
}
