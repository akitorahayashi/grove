use std::ffi::OsString;
use std::path::Path;
use std::process::ExitStatus;

use super::CommandGitClient;
use crate::AppError;
use crate::repositories::RemoteUrl;

#[derive(Debug)]
pub(crate) struct CloneInvocation {
    arguments: Vec<OsString>,
    cache: CacheDecision,
    quiet: bool,
}

impl CloneInvocation {
    pub(crate) fn new(arguments: Vec<OsString>) -> Self {
        let (cache, quiet) = analyze(&arguments);
        Self { arguments, cache, quiet }
    }

    pub(crate) fn cache(&self) -> &CacheDecision {
        &self.cache
    }

    pub(crate) fn quiet(&self) -> bool {
        self.quiet
    }
}

#[derive(Debug)]
pub(crate) enum CacheDecision {
    Eligible { url: RemoteUrl, insertion_index: usize },
    Bypassed(&'static str),
    Delegated,
}

pub(crate) trait CloneCommand {
    fn clone_command(
        &self,
        invocation: &CloneInvocation,
        reference: Option<(&Path, usize)>,
    ) -> Result<ExitStatus, AppError>;
}

impl CloneCommand for CommandGitClient {
    fn clone_command(
        &self,
        invocation: &CloneInvocation,
        reference: Option<(&Path, usize)>,
    ) -> Result<ExitStatus, AppError> {
        let mut command = self.command();
        command.arg("clone");
        for (index, argument) in invocation.arguments.iter().enumerate() {
            if let Some((reference, insertion_index)) = reference
                && index == insertion_index
            {
                command.arg("--reference-if-able").arg(reference).arg("--dissociate");
            }
            command.arg(argument);
        }
        if let Some((reference, insertion_index)) = reference
            && insertion_index == invocation.arguments.len()
        {
            command.arg("--reference-if-able").arg(reference).arg("--dissociate");
        }
        command.status().map_err(|error| AppError::git_command_failed_source("git clone", error))
    }
}

fn analyze(arguments: &[OsString]) -> (CacheDecision, bool) {
    let mut index = 0;
    let mut insertion_index = None;
    let mut bypass = None;
    let mut quiet = false;

    while index < arguments.len() {
        let Some(argument) = arguments[index].to_str() else {
            return (CacheDecision::Delegated, quiet);
        };
        if argument == "--" {
            insertion_index = Some(index);
            index += 1;
            break;
        }
        if argument == "-" || !argument.starts_with('-') {
            break;
        }

        match parse_option(arguments, index, &mut bypass, &mut quiet) {
            Some(next) => index = next,
            None => return (CacheDecision::Delegated, quiet),
        }
    }

    let operands = &arguments[index..];
    if !(1..=2).contains(&operands.len()) {
        return (CacheDecision::Delegated, quiet);
    }
    let Some(repository) = operands[0].to_str() else {
        return (CacheDecision::Delegated, quiet);
    };
    if let Some(reason) = bypass {
        return (CacheDecision::Bypassed(reason), quiet);
    }

    let decision = match RemoteUrl::new(repository) {
        Ok(url) => {
            CacheDecision::Eligible { url, insertion_index: insertion_index.unwrap_or(index) }
        }
        Err(_) => CacheDecision::Delegated,
    };
    (decision, quiet)
}

fn parse_option(
    arguments: &[OsString],
    index: usize,
    bypass: &mut Option<&'static str>,
    quiet: &mut bool,
) -> Option<usize> {
    let argument = arguments[index].to_str()?;
    if argument.starts_with("--") {
        parse_long_option(arguments, index, bypass, quiet)
    } else {
        parse_short_options(arguments, index, bypass, quiet)
    }
}

fn parse_long_option(
    arguments: &[OsString],
    index: usize,
    bypass: &mut Option<&'static str>,
    quiet: &mut bool,
) -> Option<usize> {
    let argument = arguments[index].to_str()?;
    let (name, attached) =
        argument.split_once('=').map_or((argument, None), |(name, value)| (name, Some(value)));

    if matches!(name, "--recurse-submodules" | "--recursive") {
        return Some(index + 1);
    }
    if LONG_FLAGS.contains(&name) {
        if name == "--quiet" {
            *quiet = true;
        } else if name == "--no-quiet" {
            *quiet = false;
        }
        mark_bypass(bypass, bypass_reason(name));
        return attached.is_none().then_some(index + 1);
    }
    if let Some(base) = name.strip_prefix("--no-")
        && NEGATABLE_VALUE_OPTIONS.contains(&base)
    {
        mark_bypass(bypass, bypass_reason(name));
        return attached.is_none().then_some(index + 1);
    }
    if LONG_VALUE_OPTIONS.contains(&name) {
        mark_bypass(bypass, bypass_reason(name));
        return if attached.is_some() {
            Some(index + 1)
        } else {
            arguments.get(index + 1).map(|_| index + 2)
        };
    }
    None
}

fn parse_short_options(
    arguments: &[OsString],
    index: usize,
    bypass: &mut Option<&'static str>,
    quiet: &mut bool,
) -> Option<usize> {
    let argument = arguments[index].to_str()?;
    let mut options = argument[1..].char_indices().peekable();
    options.peek()?;

    for (offset, option) in options {
        if matches!(option, 'v' | 'q' | 'n') {
            if option == 'q' {
                *quiet = true;
            }
            continue;
        }
        if matches!(option, 'l' | 's') {
            mark_bypass(bypass, Some("local or shared clone semantics"));
            continue;
        }
        if matches!(option, '4' | '6') {
            mark_bypass(bypass, Some("custom transport options"));
            continue;
        }
        if matches!(option, 'j' | 'o' | 'b' | 'u' | 'c') {
            if matches!(option, 'u' | 'c') {
                mark_bypass(bypass, Some("custom transport or Git configuration"));
            }
            let value_start = 1 + offset + option.len_utf8();
            return if value_start < argument.len() {
                Some(index + 1)
            } else {
                arguments.get(index + 1).map(|_| index + 2)
            };
        }
        return None;
    }
    Some(index + 1)
}

fn mark_bypass(target: &mut Option<&'static str>, reason: Option<&'static str>) {
    if target.is_none() {
        *target = reason;
    }
}

fn bypass_reason(option: &str) -> Option<&'static str> {
    let normalized = option.strip_prefix("--no-").unwrap_or(option.trim_start_matches("--"));
    match normalized {
        "local" | "hardlinks" | "shared" => Some("local or shared clone semantics"),
        "reference" | "reference-if-able" | "dissociate" => {
            Some("explicit object sharing semantics")
        }
        "depth"
        | "shallow-since"
        | "shallow-exclude"
        | "single-branch"
        | "filter"
        | "also-filter-submodules"
        | "revision"
        | "bundle-uri" => Some("history or object selection semantics"),
        "upload-pack" | "server-option" | "ipv4" | "ipv6" | "config" => {
            Some("custom transport or Git configuration")
        }
        _ => None,
    }
}

const LONG_FLAGS: &[&str] = &[
    "--verbose",
    "--no-verbose",
    "--quiet",
    "--no-quiet",
    "--progress",
    "--no-progress",
    "--reject-shallow",
    "--no-reject-shallow",
    "--no-checkout",
    "--checkout",
    "--bare",
    "--no-bare",
    "--mirror",
    "--no-mirror",
    "--local",
    "--no-local",
    "--no-hardlinks",
    "--hardlinks",
    "--shared",
    "--no-shared",
    "--dissociate",
    "--no-dissociate",
    "--no-recurse-submodules",
    "--no-recursive",
    "--shallow-submodules",
    "--no-shallow-submodules",
    "--single-branch",
    "--no-single-branch",
    "--tags",
    "--no-tags",
    "--also-filter-submodules",
    "--no-also-filter-submodules",
    "--remote-submodules",
    "--no-remote-submodules",
    "--sparse",
    "--no-sparse",
    "--ipv4",
    "--ipv6",
];

const LONG_VALUE_OPTIONS: &[&str] = &[
    "--jobs",
    "--template",
    "--reference",
    "--reference-if-able",
    "--origin",
    "--branch",
    "--revision",
    "--upload-pack",
    "--depth",
    "--shallow-since",
    "--shallow-exclude",
    "--separate-git-dir",
    "--ref-format",
    "--config",
    "--server-option",
    "--filter",
    "--bundle-uri",
];

const NEGATABLE_VALUE_OPTIONS: &[&str] = &[
    "jobs",
    "template",
    "reference",
    "reference-if-able",
    "origin",
    "branch",
    "revision",
    "upload-pack",
    "depth",
    "shallow-since",
    "shallow-exclude",
    "separate-git-dir",
    "ref-format",
    "config",
    "server-option",
    "filter",
    "bundle-uri",
];

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::{CacheDecision, CloneInvocation};

    fn invocation(arguments: &[&str]) -> CloneInvocation {
        CloneInvocation::new(arguments.iter().map(OsString::from).collect())
    }

    #[test]
    fn accepts_standard_git_clone_forms_for_caching() {
        for arguments in [
            vec!["https://example.com/repo.git"],
            vec!["-qvbmain", "https://example.com/repo.git", "dest"],
            vec!["--branch", "main", "--", "git@example.com:org/repo.git"],
            vec!["--mirror", "--origin=upstream", "ssh://example.com/repo"],
        ] {
            assert!(
                matches!(invocation(&arguments).cache(), CacheDecision::Eligible { .. }),
                "{arguments:?} should use the cache"
            );
        }
    }

    #[test]
    fn bypasses_cache_without_rejecting_git_owned_forms() {
        for arguments in [
            vec!["--depth", "1", "https://example.com/repo.git"],
            vec!["--filter=blob:none", "https://example.com/repo.git"],
            vec!["--reference", "seed", "https://example.com/repo.git"],
            vec!["-c", "http.extraHeader=value", "https://example.com/repo.git"],
            vec!["--future-option", "value", "https://example.com/repo.git"],
            vec![],
        ] {
            assert!(
                matches!(
                    invocation(&arguments).cache(),
                    CacheDecision::Bypassed(_) | CacheDecision::Delegated
                ),
                "{arguments:?} should bypass the cache"
            );
        }
    }

    #[test]
    fn keeps_the_existing_local_repository_cache_contract() {
        assert!(matches!(
            invocation(&["/tmp/repo.git", "dest"]).cache(),
            CacheDecision::Eligible { .. }
        ));
    }

    #[test]
    fn classifies_the_git_2_55_clone_option_surface() {
        let url = "https://example.com/repo.git";
        for flag in [
            "--verbose",
            "--no-verbose",
            "--quiet",
            "--no-quiet",
            "--progress",
            "--no-progress",
            "--reject-shallow",
            "--no-reject-shallow",
            "--no-checkout",
            "--checkout",
            "--bare",
            "--no-bare",
            "--mirror",
            "--no-mirror",
            "--recurse-submodules",
            "--recursive=lib",
            "--no-recurse-submodules",
            "--no-recursive",
            "--shallow-submodules",
            "--no-shallow-submodules",
            "--tags",
            "--no-tags",
            "--remote-submodules",
            "--no-remote-submodules",
            "--sparse",
            "--no-sparse",
        ] {
            assert!(
                matches!(invocation(&[flag, url]).cache(), CacheDecision::Eligible { .. }),
                "{flag} should remain cache eligible"
            );
        }

        for (option, value) in [
            ("--jobs", "2"),
            ("--template", "template"),
            ("--origin", "upstream"),
            ("--branch", "main"),
            ("--separate-git-dir", "gitdir"),
            ("--ref-format", "files"),
        ] {
            let attached = format!("{option}={value}");
            assert!(matches!(
                invocation(&[option, value, url]).cache(),
                CacheDecision::Eligible { .. }
            ));
            assert!(matches!(
                invocation(&[attached.as_str(), url]).cache(),
                CacheDecision::Eligible { .. }
            ));
        }

        for option in [
            "--local",
            "--no-local",
            "--hardlinks",
            "--no-hardlinks",
            "--shared",
            "--no-shared",
            "--dissociate",
            "--no-dissociate",
            "--single-branch",
            "--no-single-branch",
            "--also-filter-submodules",
            "--no-also-filter-submodules",
            "--ipv4",
            "--ipv6",
        ] {
            assert!(
                matches!(invocation(&[option, url]).cache(), CacheDecision::Bypassed(_)),
                "{option} should preserve semantics by bypassing the cache"
            );
        }

        for (option, value) in [
            ("--reference", "seed"),
            ("--reference-if-able", "seed"),
            ("--revision", "main"),
            ("--upload-pack", "git-upload-pack"),
            ("--depth", "1"),
            ("--shallow-since", "yesterday"),
            ("--shallow-exclude", "main"),
            ("--config", "core.autocrlf=false"),
            ("--server-option", "value"),
            ("--filter", "blob:none"),
            ("--bundle-uri", "https://example.com/repo.bundle"),
        ] {
            let attached = format!("{option}={value}");
            assert!(matches!(
                invocation(&[option, value, url]).cache(),
                CacheDecision::Bypassed(_)
            ));
            assert!(matches!(
                invocation(&[attached.as_str(), url]).cache(),
                CacheDecision::Bypassed(_)
            ));
        }

        for arguments in
            [vec!["-vqn", url], vec!["-j4", url], vec!["-o", "upstream", url], vec!["-bmain", url]]
        {
            assert!(matches!(invocation(&arguments).cache(), CacheDecision::Eligible { .. }));
        }
        for arguments in [
            vec!["-l", url],
            vec!["-s", url],
            vec!["-4", url],
            vec!["-6", url],
            vec!["-ugit-upload-pack", url],
            vec!["-c", "core.autocrlf=false", url],
        ] {
            assert!(matches!(invocation(&arguments).cache(), CacheDecision::Bypassed(_)));
        }
    }

    #[test]
    fn inserts_cache_arguments_before_the_operand_separator() {
        let invocation = invocation(&["--branch", "main", "--", "https://example.com/repo.git"]);

        assert!(matches!(invocation.cache(), CacheDecision::Eligible { insertion_index: 2, .. }));
    }
}
