use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::process::Command;

use super::CommandGitClient;
use super::command::command_message;
use crate::AppError;
use crate::repositories::redact_urls_for_display;

/// Variables Git lists as repository-local that still belong to the caller:
/// `GIT_CONFIG_PARAMETERS` and `GIT_CONFIG_COUNT` carry `git -c` and CI
/// credential settings, and Git exports `GIT_PREFIX` to every hook and shell
/// alias without selecting anything. `GIT_CONFIG` is not among them: it
/// replaces the file `git config` reads and writes, so the ownership probe
/// would report another file's `remote.origin.url` while `git fetch` still
/// contacts the repository's own origin, and cache refspec writes would land
/// in that file.
const PERMITTED: [&str; 3] = ["GIT_CONFIG_PARAMETERS", "GIT_CONFIG_COUNT", "GIT_PREFIX"];

impl CommandGitClient {
    /// Managed commands select their repository by working directory alone,
    /// which inherited selectors such as `GIT_DIR` or `GIT_INDEX_FILE`
    /// silently override. Git's own `--local-env-vars` list classifies them
    /// so the set tracks the installed Git rather than a copy that can drift.
    pub(super) fn reject_routing_environment(&self) -> Result<(), AppError> {
        let mut command = self.command();
        let inherited = inherited_names(&command);
        command.args(["rev-parse", "--local-env-vars"]);
        let output = command.output().map_err(AppError::git_unavailable_source)?;
        if !output.status.success() {
            return Err(AppError::git_unavailable_status(
                "git rev-parse --local-env-vars",
                redact_urls_for_display(&command_message(&output)),
                output.status.code(),
            ));
        }

        let local = String::from_utf8_lossy(&output.stdout);
        let routing = routing_overrides(&local, &inherited);
        if routing.is_empty() {
            return Ok(());
        }
        let unset = routing.iter().map(|name| format!("-u {name}")).collect::<Vec<_>>();
        Err(AppError::git_unavailable(format!(
            "inherited {} would redirect managed Git commands away from the configured \
             repositories; unset {}, for example with `env {} gv ...`, and retry",
            routing.join(", "),
            if routing.len() == 1 { "it" } else { "them" },
            unset.join(" ")
        )))
    }
}

/// The variable names a child spawned from `command` inherits: the process
/// environment adjusted by the command's own overrides.
fn inherited_names(command: &Command) -> BTreeSet<OsString> {
    let mut names = std::env::vars_os().map(|(name, _)| name).collect::<BTreeSet<_>>();
    for (name, value) in command.get_envs() {
        if value.is_some() {
            names.insert(name.to_os_string());
        } else {
            names.remove(name);
        }
    }
    names
}

fn routing_overrides<'a>(local: &'a str, inherited: &BTreeSet<OsString>) -> Vec<&'a str> {
    local
        .lines()
        .map(str::trim)
        .filter(|name| {
            !name.is_empty() && !PERMITTED.contains(name) && inherited.contains(OsStr::new(name))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::ffi::OsString;

    use super::routing_overrides;

    #[test]
    fn rejects_inherited_selectors_but_permits_injected_configuration_and_hook_context() {
        let local = "GIT_ALTERNATE_OBJECT_DIRECTORIES\nGIT_CONFIG\nGIT_CONFIG_PARAMETERS\n\
                     GIT_CONFIG_COUNT\nGIT_OBJECT_DIRECTORY\nGIT_DIR\nGIT_WORK_TREE\n\
                     GIT_INDEX_FILE\nGIT_PREFIX\nGIT_COMMON_DIR\n";
        let inherited = [
            "GIT_DIR",
            "GIT_INDEX_FILE",
            "GIT_CONFIG_PARAMETERS",
            "GIT_CONFIG_COUNT",
            "GIT_CONFIG",
            "GIT_PREFIX",
            "GIT_SSH_COMMAND",
            "HOME",
        ]
        .into_iter()
        .map(OsString::from)
        .collect::<BTreeSet<_>>();

        assert_eq!(
            routing_overrides(local, &inherited),
            ["GIT_CONFIG", "GIT_DIR", "GIT_INDEX_FILE"]
        );
    }

    #[test]
    fn variables_absent_from_the_environment_are_not_overrides() {
        assert!(routing_overrides("GIT_DIR\nGIT_WORK_TREE\n", &BTreeSet::new()).is_empty());
    }
}
