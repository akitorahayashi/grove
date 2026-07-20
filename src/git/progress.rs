#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitProgress {
    percent: Option<u8>,
    current: Option<u64>,
    total: Option<u64>,
}

impl GitProgress {
    pub fn new(percent: Option<u8>, current: Option<u64>, total: Option<u64>) -> Self {
        Self { percent, current, total }
    }

    pub fn percent(&self) -> Option<u8> {
        self.percent
    }

    pub fn current(&self) -> Option<u64> {
        self.current
    }

    pub fn total(&self) -> Option<u64> {
        self.total
    }
}

#[derive(Debug, Default)]
pub struct GitProgressParser;

impl GitProgressParser {
    pub fn parse(line: &str) -> Option<GitProgress> {
        let line = line.trim();
        let (_, detail) = line.split_once(':')?;
        let percent = parse_percent(detail);
        let (current, total) = parse_count(detail);

        if percent.is_none() && current.is_none() && total.is_none() {
            return None;
        }

        Some(GitProgress::new(percent, current, total))
    }
}

fn parse_percent(detail: &str) -> Option<u8> {
    let percent_index = detail.find('%')?;
    let before_percent = &detail[..percent_index];
    before_percent.split_whitespace().last().and_then(|value| value.parse::<u8>().ok())
}

fn parse_count(detail: &str) -> (Option<u64>, Option<u64>) {
    let Some(start) = detail.find('(') else {
        return (None, None);
    };
    let Some(end) = detail[start + 1..].find(')') else {
        return (None, None);
    };
    let count = &detail[start + 1..start + 1 + end];
    let Some((current, total)) = count.split_once('/') else {
        return (None, None);
    };

    (parse_git_count(current), parse_git_count(total))
}

fn parse_git_count(value: &str) -> Option<u64> {
    value.trim().replace(',', "").parse().ok()
}

#[cfg(test)]
mod tests {
    use super::{GitProgress, GitProgressParser};

    #[test]
    fn parses_git_percent_progress() {
        assert_eq!(
            GitProgressParser::parse("Receiving objects:  42% (128/302), 1.23 MiB | 2.00 MiB/s"),
            Some(GitProgress::new(Some(42), Some(128), Some(302)))
        );
    }

    #[test]
    fn ignores_non_progress_lines() {
        assert_eq!(GitProgressParser::parse("Cloning into 'blog'..."), None);
    }
}
