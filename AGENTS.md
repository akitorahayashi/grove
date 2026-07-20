# grove Development Overview

## Project Summary

`grove` is a Rust CLI for managing multiple Git repositories from
`grove.toml`. The CLI command is `gv`. It clones missing repositories, reports
repository state, and safely fast-forwards existing repositories' default
branches through the system `git` command. Refresh operations leave successful
working trees on their default branches.

## Architectural Highlights

- Top-level owners: `cli/` for interface adaptation, `app/` for orchestration,
  `config/` for `grove.toml`, `repositories/` for managed repository domain
  data, `git/` for the system Git boundary, and `error.rs` for
  application-wide errors.
- `src/cli/` owns parsing, terminal-safe output, progress, and command
  completion for every subcommand.
- `src/app/` owns init, refresh, status, sync, and validation orchestration and
  dependency wiring.
- `src/config/` owns discovery, include resolution, and validation.
- `src/repositories/` owns validated names, branch names, redacted URLs,
  operational paths, definitions, and target selection.
- `src/git/` owns strict Git probes and non-destructive mutation through the
  system `git` command.
- `src/zoxide/` owns optional zoxide capability checks and registration
  commands.
- `src/lib.rs` exposes only the root use-case facade and required result/error
  types. Process clients and orchestration internals are private.

## Naming Conventions

- Structs and enums: `PascalCase`.
- Functions and variables: `snake_case`.
- Modules: `snake_case`.

## Verify Commands

- Format: `cargo fmt --check`
- Lint: `cargo clippy --all-targets --all-features -- -D warnings`
- Test: `cargo test --all-targets --all-features`

## Testing Strategy

- Unit tests live beside the modules they verify.
- Integration tests live in `tests/`, with `tests/cli.rs` for CLI boundary
  behavior and `tests/library.rs` for public library boundary behavior.
- Shared integration fixtures live in `tests/harness/test_context.rs`.
- CI runs build, linting, and tests.
