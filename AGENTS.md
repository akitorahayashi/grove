# grove

## Project Overview

`grove` is a Rust CLI, invoked as `gv`, that manages the multiple Git
repositories declared in `grove.toml`. It clones missing repositories through a
local object cache, reports repository state, and fast-forwards existing
repositories' default branches through the system `git` command. The crate ships
both the binary and a library whose supported surface is the `src/lib.rs` facade.

## Directory Structure

```text
src/
  main.rs        Process entry; the sole process-termination boundary
  lib.rs         Public facade: the complete supported library API
  error.rs       AppError, its stable categories, and every error constructor
  inspection.rs  Repository readiness probing and the diagnostics the use cases share
  assets/        Templates embedded in the binary
  cli/           Clap parsing and rendering; commands/ holds one module per subcommand, tty/ the presentation over the single output sink
  app/           One use case per subcommand, plus the default dependency wiring, the external-boundary context, and the report row sync and refresh share
  cache/         The local clone cache store: entry layout, identity keying, locking, placement, seeding
  phases/        Bounded-parallel phase execution shared by the repository use cases
  config/        grove.toml discovery, include loading, and validation
  repositories/  Validated repository values: name, path, URL, branch, selection
  git/           The system `git` boundary: probes, process runner, cache entries, branch updates
  zoxide/        The optional zoxide boundary
tests/           Two integration binaries over a shared harness
```

Per-file layout and the full boundary contracts are in docs/architecture.md.

Concept owners contain their own validation, orchestration, and boundary
behavior; grove has no utility, helper, or common layer. New code belongs to the
`app/` use case that needs it, and a mechanism is promoted to a top-level owner
only when a second use case needs it or when it wraps an external tool's protocol
or on-disk format. Dependencies stay acyclic and flow downward toward `git/` and
`repositories/`: `cli/` and `app/` both depend on the shared domains, and no
domain depends on `app/` or `cli/`.

## Invariants

- Every fallible path returns `Result<_, AppError>`, built through the
  constructors in `error.rs`. A new failure mode extends that file rather than
  introducing a local error type or an error-handling crate.
- The re-exports in `src/lib.rs` are the entire supported API. Marking an item
  `pub` inside a module publishes nothing.
- Remote URLs and Git output reach a message only through
  `redact_urls_for_display`, and the CLI escapes control characters with
  `terminal_text`. `Output` is the only stdout/stderr sink and owns the color
  decision, so renderers style unconditionally.
- `phases/` owns parallel execution; a use case supplies its phases and actions
  and never spawns threads itself.
- `git stash`, `git reset --hard`, `git rebase`, `git clean`, forced checkout,
  and forced push are never issued.

## Testing

Unit tests are colocated as `#[cfg(test)] mod tests` at the foot of the module
they verify. Integration tests live in `tests/` as two Cargo binaries —
`tests/cli.rs` for CLI boundary behavior and `tests/library.rs` for the `lib.rs`
facade — sharing `tests/harness/`, which owns the temporary workspace, the
isolated cache home, the pre-wired `gv` command, and Git remote creation. Layout
and CI stages are in docs/testing.md.

## Verify Commands

```bash
just fix
just check
just test
```

Run `fix` before `check`; `check` does not modify files. `just coverage`
enforces an 86 percent floor.

## Documentation Responsibilities

- AGENTS.md — source map, cross-cutting invariants, and pointers. The orientation layer; `.claude/CLAUDE.md` is a symlink to it.
- README.md — quick start and the documentation index. The front door.
- docs/architecture.md — per-file layout, module boundaries, the public facade, data flow, and naming conventions.
- docs/config.md — the `grove.toml` schema, path resolution, includes, and validation rules.
- docs/usage.md — the complete command reference and the library API.
- docs/testing.md — test layout, coverage, and CI stages.
- CONTRIBUTING.md — development environment, toolchain, and automation policy.
