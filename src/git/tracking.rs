use crate::repositories::BranchName;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchTracking {
    MissingLocal,
    MissingRemote,
    Divergence { ahead: u32, behind: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct BranchRevisions {
    local: Option<String>,
    remote: Option<String>,
}

impl BranchRevisions {
    pub(super) fn local(&self) -> Option<&str> {
        self.local.as_deref()
    }

    pub(super) fn remote(&self) -> Option<&str> {
        self.remote.as_deref()
    }
}

pub(super) fn parse(output: &str, branch: &BranchName) -> Option<BranchRevisions> {
    let local_ref = format!("refs/heads/{branch}");
    let remote_ref = format!("refs/remotes/origin/{branch}");
    let local_children = format!("{local_ref}/");
    let remote_children = format!("{remote_ref}/");
    let mut local = None;
    let mut remote = None;

    for line in output.lines() {
        if line.is_empty() {
            return None;
        }
        let mut fields = line.split('\t');
        let reference = fields.next()?;
        let revision = fields.next()?;
        if fields.next().is_some()
            || revision.is_empty()
            || !revision.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return None;
        }

        if reference == local_ref {
            if local.replace(revision.to_string()).is_some() {
                return None;
            }
        } else if reference == remote_ref {
            if remote.replace(revision.to_string()).is_some() {
                return None;
            }
        } else if !(reference.starts_with(&local_children)
            || reference.starts_with(&remote_children))
        {
            return None;
        }
    }

    Some(BranchRevisions { local, remote })
}

#[cfg(test)]
mod tests {
    use crate::repositories::BranchName;

    use super::parse;

    #[test]
    fn parses_present_and_missing_references_and_ignores_children() {
        let branch = BranchName::new("main").unwrap();
        let both = parse(
            "refs/heads/main\tabc123\nrefs/heads/main/child\t111111\nrefs/remotes/origin/main\tdef456\n",
            &branch,
        )
        .unwrap();
        assert_eq!(both.local(), Some("abc123"));
        assert_eq!(both.remote(), Some("def456"));

        let missing_local = parse("refs/remotes/origin/main\tdef456\n", &branch).unwrap();
        assert_eq!(missing_local.local(), None);
        assert_eq!(missing_local.remote(), Some("def456"));

        let missing_remote = parse("refs/heads/main\tabc123\n", &branch).unwrap();
        assert_eq!(missing_remote.local(), Some("abc123"));
        assert_eq!(missing_remote.remote(), None);
    }

    #[test]
    fn rejects_duplicate_unexpected_and_malformed_records() {
        let branch = BranchName::new("main").unwrap();
        for output in [
            "refs/heads/main\tabc123\nrefs/heads/main\tdef456\n",
            "refs/heads/other\tabc123\n",
            "refs/heads/main\n",
            "refs/heads/main\tabc123\textra\n",
            "refs/heads/main\tnot-hex\n",
            "\n",
        ] {
            assert!(parse(output, &branch).is_none(), "accepted malformed output: {output:?}");
        }
    }
}
