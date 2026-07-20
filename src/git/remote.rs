use crate::repositories::{RemoteUrl, redact_urls_for_display};

pub fn urls_match(actual: &RemoteUrl, expected: &RemoteUrl) -> bool {
    actual.as_process_argument().trim() == expected.as_process_argument().trim()
}

pub fn redact_url_for_display(value: &str) -> String {
    redact_urls_for_display(value)
}
