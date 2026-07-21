use crate::repositories::RemoteUrl;

pub fn urls_match(actual: &RemoteUrl, expected: &RemoteUrl) -> bool {
    actual.as_process_argument().trim() == expected.as_process_argument().trim()
}
