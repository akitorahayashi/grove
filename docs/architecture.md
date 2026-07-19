# Architecture

## Intent

`grove` is a Rust CLI for managing multiple Git repositories from
`grove.toml`. The implementation follows concept-owned module boundaries:
top-level modules identify owners, and each owner keeps its contracts,
validation, and concrete implementation details inside its module.

## Design Axis

The top-level source layout is organized by owning concept.

- `src/cli` owns command-line parsing and terminal output.
- `src/app` owns dependency wiring and use-case orchestration.
- `src/config` owns `grove.toml` discovery, include resolution, path
  normalization, and validation.
- `src/repositories` owns repository definitions, names, selection, and state
  models.
- `src/git` owns the system `git` command boundary.
- `src/error.rs` owns the application-wide error type.

Boundary roles remain inside owning concepts rather than moving into generic
top-level `ports`, `adapters`, `core`, or `utils` modules.

## Current Structure

```text
src/
  error.rs
  cli/
    mod.rs
    list.rs
    status.rs
    sync.rs
  app/
    api.rs
    context.rs
    list.rs
    status.rs
    sync.rs
  config/
    discovery.rs
    file.rs
    include.rs
    resolved.rs
    validation.rs
  repositories/
    definition.rs
    name.rs
    selection.rs
    state.rs
  git/
    client.rs
    command.rs
    default_branch.rs
    remote.rs
    update.rs
    working_tree.rs
  lib.rs
  main.rs
```

`src/lib.rs` is the public library surface and CLI entrypoint export.
`src/main.rs` is the binary entrypoint that delegates to the library.

## Ownership Rules

### cli/

`src/cli` defines the `gv sync`, `gv status`, and `gv list` command surface. It
maps CLI interactions to application API calls and formats terminal output. It
does not own use-case logic, configuration invariants, repository state
inspection, or Git command behavior.

### app/

`src/app` coordinates use cases and dependency wiring. It loads validated
configuration, selects target repositories, invokes Git operations, and
aggregates command results. It does not own TOML parsing, repository name
validation, or process-level Git command execution.

### config/

`src/config` owns the `grove.toml` boundary. It discovers the active config,
loads TOML, resolves one level of includes, normalizes repository paths, and
returns a `ResolvedConfig`.

Configuration validation rejects unsupported versions, missing required fields,
nested includes, duplicate config references, duplicate repository names,
duplicate repository paths, nested repository paths, absolute repository paths,
and paths that leave the managed root.

### repositories/

`src/repositories` owns repository-domain data. `RepositoryName` validates CLI
target names. `RepositoryDefinition` represents a repository after config
resolution. Selection and state structures live beside those domain types.

### git/

`src/git` owns the system `git` command boundary. `GitClient` is the contract
used by application use cases. `CommandGitClient` invokes the installed `git`
binary with `std::process::Command`.

Git behavior relies on the user's Git configuration, including SSH settings,
credential helpers, proxy settings, Git LFS, URL rewriting, and authentication.

### error.rs

`src/error.rs` owns `AppError` when error semantics are application-wide.

## Dependency Direction

```text
main -> lib -> cli -> app
app -> config + repositories + git
config -> repositories + error
git -> error
repositories -> error
lib -> cli + app + config + repositories + git + error
```

`repositories` does not depend on `config`, `git`, `app`, or `cli`.
`config` creates repository definitions but does not execute Git commands.
`git` inspects and updates repositories but does not load `grove.toml`.

## Testing Model

- Unit tests live beside the modules they verify.
- `tests/cli.rs` verifies the CLI boundary.
- `tests/library.rs` verifies the public library boundary.
- `tests/harness/` provides shared integration fixtures and local Git remotes.
