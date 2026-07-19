pub fn urls_match(actual: &str, expected: &str) -> bool {
    actual.trim() == expected.trim()
}

pub fn redact_url_for_display(url: &str) -> String {
    redact_secret_query_parameters(&redact_authority_userinfo(&escape_control_characters(url)))
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
    use super::redact_url_for_display;

    #[test]
    fn display_url_redacts_http_userinfo_and_secret_query_values() {
        assert_eq!(
            redact_url_for_display(
                "https://user:ghp_secret@example.com/org/repo.git?access_token=token&branch=main&api_key=key"
            ),
            "https://[redacted]@example.com/org/repo.git?access_token=[redacted]&branch=main&api_key=[redacted]"
        );
    }

    #[test]
    fn display_url_redacts_userinfo_for_non_http_hierarchical_urls() {
        assert_eq!(
            redact_url_for_display("ssh://user:secret@example.com/repo.git"),
            "ssh://[redacted]@example.com/repo.git"
        );
        assert_eq!(
            redact_url_for_display("ftp://user:password@example.com/repo.git"),
            "ftp://[redacted]@example.com/repo.git"
        );
    }

    #[test]
    fn display_url_escapes_control_characters() {
        let displayed = redact_url_for_display("https://example.com/org/repo.git\n\t\u{1b}[31m");

        assert_eq!(displayed, "https://example.com/org/repo.git\\n\\t\\u{1b}[31m");
        assert!(!displayed.chars().any(char::is_control));
    }
}
