use std::fmt;

use crate::AppError;

#[derive(Clone, PartialEq, Eq)]
pub struct RemoteUrl(String);

impl RemoteUrl {
    pub fn new(value: &str) -> Result<Self, AppError> {
        if value.is_empty() || value.chars().any(char::is_control) {
            Err(AppError::invalid_arguments("invalid repository URL"))
        } else {
            Ok(Self(value.to_string()))
        }
    }

    pub(crate) fn from_git(value: String) -> Self {
        Self(value)
    }

    pub fn as_process_argument(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RemoteUrl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&redact_urls_for_display(&self.0))
    }
}

impl fmt::Debug for RemoteUrl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("RemoteUrl").field(&self.to_string()).finish()
    }
}

pub(crate) fn redact_urls_for_display(value: &str) -> String {
    let escaped = escape_control_characters(value);
    let mut output = String::with_capacity(escaped.len());
    let mut remaining = escaped.as_str();

    while let Some(start) = find_url_start(remaining) {
        output.push_str(&remaining[..start]);
        let candidate = &remaining[start..];
        let end = candidate
            .find(|character: char| {
                character.is_whitespace() || matches!(character, '\'' | '"' | ')' | ']')
            })
            .unwrap_or(candidate.len());
        let url = &candidate[..end];
        output.push_str(&redact_secret_query_parameters(&redact_authority_userinfo(url)));
        remaining = &candidate[end..];
    }
    output.push_str(remaining);
    output
}

fn find_url_start(value: &str) -> Option<usize> {
    ["https://", "http://", "ssh://", "ftp://"]
        .into_iter()
        .filter_map(|scheme| value.find(scheme))
        .min()
}

fn escape_control_characters(value: &str) -> String {
    let mut escaped = String::new();
    for character in value.chars() {
        if character.is_control() {
            escaped.extend(character.escape_default());
        } else {
            escaped.push(character);
        }
    }
    escaped
}

fn redact_authority_userinfo(value: &str) -> String {
    let Some(scheme_end) = value.find("://") else {
        return value.to_string();
    };
    let authority_start = scheme_end + 3;
    let authority = &value[authority_start..];
    let authority_end =
        authority.find(|character| ['/', '?', '#'].contains(&character)).unwrap_or(authority.len());
    let Some(userinfo_end) = authority[..authority_end].rfind('@') else {
        return value.to_string();
    };

    format!(
        "{}[redacted]@{}",
        &value[..authority_start],
        &value[authority_start + userinfo_end + 1..]
    )
}

fn redact_secret_query_parameters(value: &str) -> String {
    let Some(query_start) = value.find('?') else {
        return value.to_string();
    };
    let query_value_start = query_start + 1;
    let fragment_start = value[query_value_start..]
        .find('#')
        .map(|index| query_value_start + index)
        .unwrap_or(value.len());
    let query = &value[query_value_start..fragment_start];
    let mut redacted = String::from(&value[..query_value_start]);

    for (index, parameter) in query.split('&').enumerate() {
        if index > 0 {
            redacted.push('&');
        }
        let (key, has_value) =
            parameter.split_once('=').map_or((parameter, false), |(key, _)| (key, true));
        if has_value && is_secret_query_key(key) {
            redacted.push_str(key);
            redacted.push_str("=[redacted]");
        } else {
            redacted.push_str(parameter);
        }
    }

    redacted.push_str(&value[fragment_start..]);
    redacted
}

fn is_secret_query_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase();
    normalized.contains("token")
        || normalized.contains("password")
        || normalized.contains("passwd")
        || normalized.contains("secret")
        || normalized.contains("auth")
        || normalized == "key"
        || normalized.ends_with("_key")
        || normalized.ends_with("-key")
}

#[cfg(test)]
mod tests {
    use super::{RemoteUrl, redact_urls_for_display};

    #[test]
    fn display_and_debug_redact_credentials_and_secret_queries() {
        let url = RemoteUrl::new(
            "https://user:secret@example.com/repo.git?access_token=value&branch=main",
        )
        .unwrap();

        assert_eq!(
            url.to_string(),
            "https://[redacted]@example.com/repo.git?access_token=[redacted]&branch=main"
        );
        assert!(!format!("{url:?}").contains("secret"));
        assert!(!format!("{url:?}").contains("value"));
    }

    #[test]
    fn redacts_urls_embedded_in_external_diagnostics() {
        let displayed = redact_urls_for_display(
            "fatal: https://user:secret@example.com/repo?password=value\n\u{1b}[31mfailed",
        );

        assert!(displayed.contains("https://[redacted]@example.com/repo?password=[redacted]"));
        assert!(!displayed.contains("secret"));
        assert!(!displayed.contains("value"));
        assert!(!displayed.chars().any(char::is_control));
    }
}
