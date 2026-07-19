pub(super) fn parse_origin_head(output: &str) -> Option<String> {
    let branch = output.trim().strip_prefix("origin/")?;
    if branch.is_empty() { None } else { Some(branch.to_string()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_origin_head() {
        assert_eq!(parse_origin_head("origin/main\n").as_deref(), Some("main"));
    }

    #[test]
    fn rejects_unexpected_symbolic_ref() {
        assert_eq!(parse_origin_head("upstream/main\n"), None);
    }
}
