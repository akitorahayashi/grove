# grove

`grove` manages multiple Git repositories from `grove.toml`. The CLI command is `gv`.

## Usage

```bash
gv --version
gv init
gv validate

gv status
gv status frontend
gv status --fetch

gv sync
gv sync frontend
gv sync -z
gv sync --dry-run

gv refresh
gv refresh frontend
gv refresh --dry-run
gv --config ~/workspace/grove.toml status
```

## Configuration

`grove.toml` declares repository names, target paths, and clone URLs.

```toml
version = 1

[repos.frontend]
url = "git@github.com:company/frontend.git"
default_branch = "main"

[repos.backend]
path = "services/backend"
url = "git@github.com:company/backend.git"

[repos."company.service"]
url = "git@github.com:company/service.git"
```

Repository names are the direct table keys under `repos`. The `path` value is
optional and defaults to the repository name. Explicit paths are resolved
relative to the `grove.toml` file that defines the repository. Absolute paths
and paths that leave the canonical grove root are rejected. Symlinks are valid
when their canonical targets remain inside the root. Symlink aliases share one
operational identity for duplicate and nested path validation. Repository names
with `.` use quoted table keys.

`default_branch` is optional. An explicitly configured branch takes precedence
over `origin/HEAD`; `origin/HEAD` is used only when the field is absent. Branch
names are validated as Git refs without invoking Git, so `gv validate` remains
independent of installed external tools.

Root configuration files can include one level of child configuration files.

```toml
version = 1

include = [
  "personal/grove.toml",
  "work/grove.toml",
]
```

Child configuration files define repositories and cannot include other
configuration files.

`gv validate` loads `grove.toml`, resolves includes, and validates configuration
without inspecting repository working trees or requiring network access.

## Status Behavior

`gv status`, with the aliases `gv st` and `gv ts`, reports managed repository
state as a table, or as a single-repository detail view when one repository is
named. It never mutates Git state.

`gv status --fetch` refreshes remote-tracking state before reporting. The fetch
runs independent repositories concurrently with at most eight live tasks, while
linked worktrees sharing a Git common directory remain serialized, matching sync
and refresh. Without `--fetch`, repositories are inspected serially. Report
entries preserve selection order in either case.

## Sync Behavior

`gv sync` clones missing repositories and safely updates existing repositories'
default branches. Existing repositories are updated through system `git`
commands. Independent repository tasks run concurrently with at most eight live
tasks. Linked worktrees that share a Git common directory remain serialized.

The update flow is:

```text
git fetch origin --prune
git switch <default-branch>
git merge --ff-only origin/<default-branch>
git switch <original-branch>
```

The current branch and clean working-tree preconditions are checked again at
the mutation boundary. Every failure after a successful switch attempts to
restore the original branch. A completed fast-forward followed by restoration
failure is reported as an update with a restoration error and exits
unsuccessfully.

The following operations are never performed automatically:

- `git stash`
- `git reset --hard`
- `git rebase`
- `git clean`
- forced checkout
- forced push

`gv sync --register-zoxide` and its short form `gv sync -z` register synced
repositories with zoxide. Skipped and blocked repositories are not registered,
and dry runs only report the repositories that would be registered. Registration
uses one initial zoxide database snapshot and at most one final snapshot.

## Refresh Behavior

`gv refresh`, with the alias `gv rf`, updates repositories that already exist
locally and leaves each successful working tree on its default branch. Missing
repositories are blocked with guidance to run `gv sync`; they are not cloned.
Independent repository tasks run concurrently with at most eight live tasks,
while linked worktrees sharing a Git common directory remain serialized. Multiple
selected linked worktrees that would finish on the same default branch are
blocked before switching, because Git permits a branch to be checked out by only
one linked worktree at a time.

The refresh flow is:

```text
git fetch origin --prune
git switch <default-branch>
git merge --ff-only origin/<default-branch>
```

The switch is omitted when the default branch is already checked out. Ahead or
diverged default branches are blocked before switching. Equal and behind
branches are accepted, and the previous branch is neither restored nor deleted.
A failure after a successful switch also leaves the default branch checked out
and is reported as a blocked refresh that already switched branches.

`gv refresh --dry-run` performs local validation without fetching or mutating
Git state. Divergence in a dry run reflects the locally available
remote-tracking refs, which may be older than the remote repository.

## Requirements

Linux and macOS are the supported platforms. Windows is unsupported.

- Git 2.23.0 or newer is required.
- zoxide is optional. Registration requires `zoxide query --list --all` and
  `zoxide add`.

Release builds contain the Linux x86_64, Linux aarch64, macOS x86_64, and macOS
aarch64 binaries. Checksums, signatures, and attestations are not published.

## Library API

The supported Rust API is the crate-root facade: `cli`, `refresh`, `status`,
`sync`, and `validate`, plus their report, outcome, and error types. `sync` and
`refresh` take option structs: `sync(config, targets, SyncOptions)` and
`refresh(config, targets, RefreshOptions)`. `SyncOptions` carries the dry-run
and zoxide-registration flags, so library callers reach the same zoxide report
as the CLI. `cli` returns an `ExitCode` and does not terminate its host process.

```rust
let report = grove::validate(Some("/workspace/grove.toml".into()))?;
println!("{} repositories", report.repository_count());
# Ok::<(), grove::AppError>(())
```
