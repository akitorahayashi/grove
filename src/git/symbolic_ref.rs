use crate::repositories::BranchName;

/// The branch a full `git symbolic-ref` target names inside `namespace`.
/// Targets are read unabbreviated because `--short` prints the shortest
/// unambiguous spelling, which a tag sharing the branch's name turns into
/// `heads/main` or `remotes/origin/main`. Only the line terminator is removed:
/// a branch name may contain non-ASCII whitespace.
pub(super) fn branch(output: &[u8], namespace: &str) -> Option<BranchName> {
    let line = std::str::from_utf8(output.strip_suffix(b"\n")?).ok()?;
    BranchName::new(line.strip_prefix(namespace)?).ok()
}

#[cfg(test)]
mod tests {
    use super::branch;

    #[test]
    fn reads_the_branch_inside_the_namespace() {
        assert_eq!(
            branch(b"refs/remotes/origin/release/stable\n", "refs/remotes/origin/")
                .map(|branch| branch.as_str().to_string()),
            Some("release/stable".to_string())
        );
        assert_eq!(
            branch("refs/heads/caf\u{e9}\u{a0}\n".as_bytes(), "refs/heads/")
                .map(|branch| branch.as_str().to_string()),
            Some("caf\u{e9}\u{a0}".to_string())
        );
    }

    #[test]
    fn rejects_other_namespaces_and_malformed_lines() {
        for output in [
            &b"refs/remotes/upstream/main\n"[..],
            b"origin/main\n",
            b"refs/remotes/origin/main",
            b"refs/remotes/origin/main\nrefs/remotes/origin/next\n",
            b"refs/remotes/origin/\n",
        ] {
            assert_eq!(branch(output, "refs/remotes/origin/"), None, "{output:?}");
        }
    }
}
