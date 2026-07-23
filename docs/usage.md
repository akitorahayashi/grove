# Usage

## Commands

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

gv clone git@github.com:company/frontend.git
gv clone git@github.com:company/frontend.git frontend

gv cache list
gv cache clean
gv cache clean frontend
```

`gv init`, with the alias `gv i`, creates `grove.toml` in the current directory.
`gv validate`, with the alias `gv vl`, checks `grove.toml` without inspecting
repositories.

`gv clone`, with the alias `gv cl`, and `gv cache`, with the alias `gv c`, expose the
local clone cache from the command line. `gv cache list`, with the alias `gv c ls`,
and `gv cache clean`, with the alias `gv c cln`, operate on cache entries.

Configuration file schema and validation rules are documented in
[configuration](config.md).

## Status

`gv status`, with the aliases `gv st` and `gv ts`, reports managed repository
state as a table, or as a single-repository detail view when one repository is
named. Plain `gv status` is read-only; neither it nor `--fetch` changes the
working tree or the checked-out branch.

`gv status --fetch` refreshes remote-tracking state before reporting.
Repositories are inspected concurrently with at most eight live tasks in both
modes. Fetching status serializes linked worktrees sharing a Git common
directory, matching sync and refresh. Report entries preserve selection order.

## Sync

`gv sync`, with the alias `gv s`, clones missing repositories and safely updates
existing repositories' default branches. Missing repositories are cloned through the local clone cache
(see Clone Cache). Existing repositories are updated through system `git`
commands, and an existing repository grove could reach whose URL has no cache
entry seeds one from its local objects (see Clone Cache). Independent repository
tasks run concurrently with at most eight live tasks. Linked worktrees that
share a Git common directory remain serialized.

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

## Refresh

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

## Clone cache

`gv sync` and `gv clone` clone through a local object cache so repeat clones of
a previously seen URL transfer only the difference from the remote. The cache
lives under `${XDG_CACHE_HOME}/grove`, falling back to `${HOME}/.cache/grove`;
when neither variable is set, the command fails rather than choosing another
location.

Each cache entry is a bare, single-branch repository holding the default branch
history, keyed by the verbatim remote URL. The `git@` and `https://` forms of
one repository are distinct entries. Placement runs
`git clone --reference <entry> --dissociate` against the real remote, so
`origin` points at the requested URL and the placed clone is self-contained;
removing the cache never affects existing clones. Because Git objects are
content-addressed and refs always come from the real remote, a stale or narrow
entry only reduces transfer and never yields an incorrect clone.

On each use the entry is created when absent, refreshed when present, rebuilt
when unusable, and repointed when a different default branch is requested.
Within one process, tasks sharing a URL are serialized; concurrent `gv`
processes operating on the same entry are not coordinated.

`gv sync` also seeds the cache from repositories already on disk. After the
update phase, each reachable existing repository whose URL has no entry — one
that was fetched, or that was left untouched for a dirty working tree or a
detached HEAD — has an entry built from its local objects, borrowed and
dissociated like any placement and tracking the remote's default branch, so
later clones of that URL transfer only the difference. Each URL is seeded once.
Seeding runs as its own phase and is best-effort: a failure is reported as a
note without changing the repository's own result.

`gv clone <url> [dest]` clones a single repository through the cache without
reading or writing `grove.toml`. `dest` defaults to the final URL path segment
with a trailing `.git` removed. An existing non-empty destination is rejected,
and `--config` is not accepted. It is a cache-accelerated `git clone`, not a
reimplementation of every `git clone` option.

`gv cache list` reports cached repositories in `URL` and `UPDATED` columns. `gv
cache clean` removes every entry; `gv cache clean <repo>...` removes the entries
backing the named configured repositories.

## Requirements

Linux and macOS are the supported platforms. Windows has no runtime, test, CI,
or release support.

- Git 2.23.0 or newer is required.
- zoxide is optional. Registration requires `zoxide query --list --all` and
  `zoxide add`.

Release builds contain the Linux x86_64, Linux aarch64, macOS x86_64, and macOS
aarch64 binaries. Checksums, signatures, and attestations are not published.

## Library API

The supported Rust API is the crate-root facade: `cli`, `clone`, `refresh`,
`status`, `sync`, and `validate`, plus their report, outcome, and error types.
`sync` and `refresh` take option structs: `sync(config, targets, SyncOptions)`
and `refresh(config, targets, RefreshOptions)`. `SyncOptions` carries the
dry-run and zoxide-registration flags, so library callers reach the same zoxide
report as the CLI. `clone(url, destination)` clones a single URL through the
cache and returns a `CloneReport` carrying the resulting `CacheOutcome`. Sync
and refresh report entries expose the structured blocked-reason detail through
`blocked_details()`, returning `BlockedReasonDetails` such as a remote-URL
mismatch's actual and expected values, so callers reproduce the CLI diagnostics.
`cli` returns an `ExitCode` and does not terminate its host process.

```rust
let report = grove::validate(Some("/workspace/grove.toml".into()))?;
println!("{} repositories", report.repository_count());
# Ok::<(), grove::AppError>(())
```
