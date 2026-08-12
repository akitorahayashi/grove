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

    /// Whether two URLs name the same remote verbatim. This is exact, unlike
    /// `identity`: a configured URL and a repository's `origin` must agree on
    /// the transport too, so a switch from `https://` to `git@` is reported as
    /// a mismatch rather than silently accepted.
    pub(crate) fn matches(&self, other: &RemoteUrl) -> bool {
        self.0.trim() == other.0.trim()
    }

    /// The repository identity the cache keys on: `host[:port]/path` with the
    /// host lowercased and userinfo, query, fragment, leading and trailing
    /// slashes, and a trailing `.git` dropped, so the `git@` and `https://`
    /// forms of one repository share it and embedded credentials never become
    /// part of it. This follows Git hosting conventions rather than proven
    /// equivalence — on plain SSH servers, userinfo or a home-relative path
    /// can distinguish repositories — so the cache treats a shared identity
    /// as a hint and rebuilds an entry whose refresh fails (see
    /// `crate::cache`). `file://` URLs reduce to their path; local paths stay
    /// verbatim because on a filesystem `repo` and `repo.git` are genuinely
    /// different directories.
    pub(crate) fn identity(&self) -> String {
        let value = self.0.as_str();
        if let Some((scheme, remainder)) = value.split_once("://") {
            let authority_end = remainder.find(['/', '?', '#']).unwrap_or(remainder.len());
            let (authority, resource) = remainder.split_at(authority_end);
            let host = authority.rsplit_once('@').map_or(authority, |(_, host)| host);
            if scheme.eq_ignore_ascii_case("file") {
                // Git resolves file:// URLs by their path alone, discarding
                // the authority, so a host or userinfo never distinguishes
                // repositories and must not reach the identity.
                return resource.to_string();
            }
            return join_identity(host, resource);
        }

        // scp-like syntax: a ':' before the first '/' separates host and path
        // (git's own rule); a leading path segment such as `./` never
        // contains one, so local paths fall through verbatim.
        let head = value.split('/').next().unwrap_or_default();
        if head.contains(':')
            && let Some((before, path)) = value.split_once(':')
            && !before.is_empty()
        {
            let host = before.rsplit_once('@').map_or(before, |(_, host)| host);
            return join_identity(host, path);
        }

        value.to_string()
    }
}

fn join_identity(host: &str, resource: &str) -> String {
    let path = resource.split(['?', '#']).next().unwrap_or_default();
    let trimmed = path.trim_matches('/');
    let path = trimmed.strip_suffix(".git").unwrap_or(trimmed).trim_end_matches('/');
    let host = host.to_ascii_lowercase();
    if path.is_empty() { host } else { format!("{host}/{path}") }
}

impl fmt::Display for RemoteUrl {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&redact_url(&self.0))
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
    let bytes = value.as_bytes();
    for (scheme_end, _) in value.match_indices("://") {
        let mut start = scheme_end;
        while start > 0 && is_scheme_character(bytes[start - 1]) {
            start -= 1;
        }
        if start < scheme_end && bytes[start].is_ascii_alphabetic() {
            return Some(start);
        }
    }
    None
}

fn is_scheme_character(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.')
}

fn redact_url(value: &str) -> String {
    let escaped = escape_control_characters(value);
    redact_secret_query_parameters(&redact_authority_userinfo(&escaped))
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
    let normalized = decode_query_key(key).to_ascii_lowercase();
    normalized.contains("token")
        || normalized.contains("password")
        || normalized.contains("passwd")
        || normalized.contains("secret")
        || normalized.contains("auth")
        || normalized == "key"
        || normalized.ends_with("_key")
        || normalized.ends_with("-key")
}

fn decode_query_key(key: &str) -> String {
    let bytes = key.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && let Some(value) = bytes
                .get(index + 1)
                .and_then(|high| hex_value(*high))
                .zip(bytes.get(index + 2).and_then(|low| hex_value(*low)))
                .map(|(high, low)| high << 4 | low)
        {
            decoded.push(value);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
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

    #[test]
    fn recognizes_generic_case_insensitive_uri_schemes() {
        for (input, expected) in [
            (
                "fatal: HTTPS://user:secret@example.com/repo.git",
                "HTTPS://[redacted]@example.com/repo.git",
            ),
            (
                "fatal: git://user:secret@example.com/repo.git",
                "git://[redacted]@example.com/repo.git",
            ),
            (
                "fatal: file://user:secret@example.com/repo.git",
                "file://[redacted]@example.com/repo.git",
            ),
            (
                "fatal: custom+ssh://user:secret@example.com/repo.git",
                "custom+ssh://[redacted]@example.com/repo.git",
            ),
        ] {
            let displayed = redact_urls_for_display(input);
            assert!(displayed.contains(expected), "{displayed}");
            assert!(!displayed.contains("secret"), "{displayed}");
        }
    }

    #[test]
    fn recognizes_encoded_secret_query_keys_without_rewriting_keys() {
        let displayed = redact_urls_for_display(
            "HTTPS://example.com/repo?access%5Ftoken=value&%50ASSWORD=secret&branch=main",
        );

        assert_eq!(
            displayed,
            "HTTPS://example.com/repo?access%5Ftoken=[redacted]&%50ASSWORD=[redacted]&branch=main"
        );
    }

    #[test]
    fn preserves_surrounding_diagnostic_punctuation() {
        let displayed = redact_urls_for_display(
            "failed (HTTPS://user:secret@example.com/repo.git?token=value), retry",
        );

        assert_eq!(
            displayed,
            "failed (HTTPS://[redacted]@example.com/repo.git?token=[redacted]), retry"
        );
    }

    #[test]
    fn identity_unifies_transports_of_one_repository() {
        for url in [
            "https://github.com/Org/Repo.git",
            "HTTPS://GITHUB.COM/Org/Repo",
            "https://user:secret@github.com/Org/Repo.git?access_token=value#fragment",
            "ssh://git@github.com/Org/Repo.git",
            "git@github.com:Org/Repo.git",
            "git@GitHub.com:/Org/Repo/",
        ] {
            assert_eq!(RemoteUrl::new(url).unwrap().identity(), "github.com/Org/Repo", "{url}");
        }
    }

    #[test]
    fn identity_keeps_ports_distinct() {
        assert_eq!(
            RemoteUrl::new("ssh://git@example.com:2222/org/repo.git").unwrap().identity(),
            "example.com:2222/org/repo"
        );
    }

    #[test]
    fn identity_keeps_local_paths_verbatim() {
        for path in ["/srv/git/repo.git", "./relative/repo", "../repo.git/", "seeds/blog"] {
            assert_eq!(RemoteUrl::new(path).unwrap().identity(), path, "{path}");
        }
    }

    #[test]
    fn identity_reduces_file_urls_to_their_verbatim_path() {
        for url in [
            "file:///srv/git/repo.git",
            "file://localhost/srv/git/repo.git",
            "file://example.com/srv/git/repo.git",
            "file://user:secret@example.com/srv/git/repo.git",
        ] {
            assert_eq!(RemoteUrl::new(url).unwrap().identity(), "/srv/git/repo.git", "{url}");
        }
    }

    #[test]
    fn identity_treats_colons_after_the_first_path_segment_as_local() {
        assert_eq!(RemoteUrl::new("./odd:name/repo").unwrap().identity(), "./odd:name/repo");
    }

    #[test]
    fn preserves_scp_like_urls_while_redacting_their_secret_queries() {
        let url =
            RemoteUrl::new("git@example.com:org/repo.git?access_token=secret&branch=main").unwrap();

        assert_eq!(
            url.to_string(),
            "git@example.com:org/repo.git?access_token=[redacted]&branch=main"
        );
    }
}
