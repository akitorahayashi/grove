# Architecture

## Intent

`grove` manages repositories declared in `grove.toml`. Concept owners contain
their own validation, orchestration, and external-boundary behavior; generic
utility or process layers are absent.

## Source layout

```text
src/
  main.rs
  lib.rs
  error.rs
  assets/
    grove.toml.tpl
  cli/
    mod.rs
    output.rs
    commands/
      mod.rs
      cache/
        mod.rs
      clone/
        mod.rs
      init.rs
      refresh/
        mod.rs
      status.rs
      sync/
        mod.rs
      validate.rs
    tty/
      mod.rs
      progress.rs
      report.rs
      table.rs
  app/
    api.rs
    cache/
      mod.rs
    clone/
      mod.rs
    context.rs
    events.rs
    init.rs
    inspection.rs
    phases.rs
    report.rs
    refresh/
      check.rs
      fetch.rs
      mod.rs
      report.rs
      update.rs
    status.rs
    validate.rs
    sync/
      check.rs
      mod.rs
      prepare.rs
      report.rs
      update.rs
      zoxide.rs
    workers.rs
  config/
    discovery.rs
    file.rs
    include.rs
    mod.rs
    resolved.rs
    validation.rs
  repositories/
    branch_name.rs
    definition.rs
    mod.rs
    name.rs
    path.rs
    selection.rs
    url.rs
  git/
    client.rs
    command.rs
    default_branch.rs
    mod.rs
    progress.rs
    remote.rs
    update.rs
  zoxide/
    client.rs
    command.rs
    mod.rs
```

## Boundaries

`cli` owns Clap parsing, stream selection, terminal-safe text, styling, progress,
and command completion. Subcommand implementations live under `commands`, while
`tty` owns the terminal presentation vocabulary built on the shared `output`
sink. The progress pump, the blocked-reason detail rendering, and the
repository-count wording are shared across the phase-emitting commands rather
than duplicated per command. The column-aligned table is shared between the
status and cache listings, which emit styling unconditionally and let `output`
strip ANSI when the destination or environment calls for plain text.
Subcommands return completion or error values. The
crate-root `cli` function returns `ExitCode`; `main` is the sole process
termination boundary. Output write failures propagate, and a closed stdout pipe
has non-panicking handling.

`app` owns the five use cases and default dependency wiring. Sync has check,
clone/fetch preparation, update, and optional zoxide phases. Refresh has check,
fetch, and default-branch refresh phases. Status inspects repositories serially,
or, with `--fetch`, through the same bounded parallel workers keyed by Git common
directory. Results retain selection order.
Worker execution is bounded by the selected work, available parallelism, and a
ceiling of eight. Shared Git common directories are serialized, and refresh
blocks selected linked worktrees that would finish on the same default branch.
Worker panic and channel disconnects become application errors. Progress events
and the phase skeleton are shared: `events` owns the phase-generic event, sink,
and progress adapter, and `phases` owns the check and worker phase envelopes.
Each use case supplies its own phase marker, per-repository action, and change
predicate. `inspection` owns repository readiness probing and the canonical
diagnostics for the conditions the use cases share, so their reason vocabularies
map from one probe and their shared messages cannot drift. `report` owns the
report entry, generic over each use case's outcome vocabulary, and the shared
blocked-reason detail.

`config` discovers the root file, resolves one include level, decodes TOML, and
validates the complete catalog without invoking Git or zoxide. It rejects schema
violations, unsupported versions, duplicate or nested includes, invalid names
and branch refs, duplicate or nested repository identities, absolute paths, and
paths outside the canonical grove root.

`repositories` owns validated repository values. `RemoteUrl` exposes raw text
only through its process-argument accessor; `Display` and `Debug` are redacted.
Repository path resolution canonicalizes the deepest existing ancestor and
appends the nonexistent suffix. In-root aliases resolve to one operational
identity while retaining the configured display path.

`git` owns Git availability, strict probe grammars, progress parsing, clone and
fetch execution, and default-branch mutation. Git 2.23.0 is the minimum because
updates use `git switch`. Expected absence statuses are declared per probe;
other failures and malformed output remain errors. Sync records restoration
separately from the primary update result. Refresh leaves successful worktrees
on the default branch and reports update failures after a successful switch with
the previous branch preserved.

The clone boundary revalidates the destination's existing ancestor immediately
before creating directories and invoking Git. A filesystem actor can still
replace components between validation and mutation; the standard filesystem
API provides no portable atomic confinement primitive for this residual race.

`zoxide` owns optional capability checks and command execution. Registration
uses an initial database snapshot, adds missing entries with per-path outcomes,
and uses one final snapshot to classify successful adds.

## Public facade

`src/lib.rs` exports `cli`, `refresh`, `status`, `sync`, and `validate`. It also
exports the reports, outcomes, and `AppError` needed to consume those calls.
Owner modules, process clients, dependency traits, events, and workers remain
private.

## Data flow

```text
CLI or root facade
  -> config discovery, include loading, and validation
  -> repository selection
  -> app use case
  -> Git and optional zoxide boundaries
  -> typed report
  -> terminal-safe CLI rendering or library caller
```

## Platform and automation

Linux and macOS are the complete supported platform set. Windows has no runtime,
test, CI, or release support. CI obtains Rust 1.90.0 and its components from
`rust-toolchain.toml`, while mise owns pinned development tools. GitHub Actions
are selected by immutable commit identifiers and release permissions are scoped
to release jobs. Releases contain four binaries and no checksum, signature, or
attestation assets.
