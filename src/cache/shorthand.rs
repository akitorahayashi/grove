//! Naming a cache entry by the trailing segments of its identity, so `gv clone
//! owner/repo` reaches `github.com/owner/repo` without spelling the host or the
//! transport.

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Shorthand {
    text: String,
    segments: Vec<String>,
}

impl Shorthand {
    /// Accepts only operands Git cannot read as a URL or an explicit path. A
    /// `:` anywhere is rejected because Git reads `host:path` as scp syntax.
    /// Whether a relative operand names an existing local repository depends on
    /// the working directory and is the caller's check.
    pub(crate) fn parse(value: &str) -> Option<Self> {
        if value.starts_with(['-', '~'])
            || value.contains(':')
            || value.chars().any(char::is_control)
        {
            return None;
        }
        let segments = comparable_segments(value.trim_end_matches('/'));
        if segments.iter().any(|segment| matches!(segment.as_str(), "" | "." | "..")) {
            return None;
        }
        Some(Self { text: value.to_string(), segments })
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.text
    }

    /// Whole segments only, so `lo` never reaches `blog`. A trailing `.git` is
    /// ignored on both sides because local-path identities keep it verbatim.
    pub(crate) fn matches(&self, identity: &str) -> bool {
        let segments = comparable_segments(identity.trim_matches('/'));
        segments.ends_with(&self.segments)
    }
}

fn comparable_segments(value: &str) -> Vec<String> {
    let mut segments = value.split('/').map(str::to_string).collect::<Vec<_>>();
    if let Some(last) = segments.last_mut()
        && let Some(stripped) = last.strip_suffix(".git")
    {
        *last = stripped.to_string();
    }
    segments
}

#[cfg(test)]
mod tests {
    use super::Shorthand;

    #[test]
    fn rejects_operands_git_reads_as_urls_or_explicit_paths() {
        for operand in [
            "",
            "https://example.com/owner/repo.git",
            "git@example.com:owner/repo.git",
            "example.com:owner/repo",
            "/srv/repo",
            "./repo",
            "../repo",
            "~/repo",
            "owner//repo",
            "owner/../repo",
            ".git",
            "-repo",
            "owner/re\npo",
        ] {
            assert!(Shorthand::parse(operand).is_none(), "{operand:?} should not be a shorthand");
        }
    }

    #[test]
    fn matches_trailing_identity_segments() {
        for (operand, identity) in [
            ("repo", "github.com/owner/repo"),
            ("owner/repo", "github.com/owner/repo"),
            ("github.com/owner/repo", "github.com/owner/repo"),
            ("owner/repo/", "github.com/owner/repo"),
            ("repo.git", "github.com/owner/repo"),
            ("blog", "/srv/remotes/blog.git"),
            ("remotes/blog", "/srv/remotes/blog.git"),
        ] {
            let shorthand = Shorthand::parse(operand).unwrap();
            assert!(shorthand.matches(identity), "{operand} should match {identity}");
        }
    }

    #[test]
    fn never_matches_partial_or_differently_cased_segments() {
        for (operand, identity) in [
            ("lo", "github.com/owner/blog"),
            ("owner", "github.com/owner/repo"),
            ("other/repo", "github.com/owner/repo"),
            ("Repo", "github.com/owner/repo"),
            ("example.com/owner/repo", "github.com/owner/repo"),
        ] {
            let shorthand = Shorthand::parse(operand).unwrap();
            assert!(!shorthand.matches(identity), "{operand} should not match {identity}");
        }
    }
}
