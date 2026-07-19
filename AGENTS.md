# grove Development Overview

## Project Summary
`grove` is a Rust CLI for managing multiple Git repositories from
`grove.toml`. The CLI command is `gv`. It clones missing repositories, reports
repository state, and safely fast-forwards existing repositories' default
branches through the system `git` command.

## Tech Stack
- Language: Rust
- CLI parsing: `clap`
- Serialization: `serde`, `serde_json`, `toml`
- Development dependencies:
  - `assert_cmd`
  - `assert_fs`
  - `predicates`
  - `serial_test`
  - `tempfile`

## Coding Standards
- Formatter: `rustfmt` with a maximum line width of 100 characters,
  crate-level import granularity, and grouped imports.
- Linter: `clippy` with warnings treated as errors (`-D warnings`).

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

## Architectural Highlights
- Top-level owners: `cli/` for interface adaptation, `app/` for orchestration,
  `config/` for `grove.toml`, `repositories/` for managed repository domain
  data, `git/` for the system Git boundary, and `error.rs` for
  application-wide errors.
- `src/cli/` owns only parsing and terminal output for `sync`, `status`, and
  `list`.
- `src/app/` owns use-case orchestration and dependency wiring.
- `src/config/` owns discovery, include resolution, validation, and path
  normalization.
- `src/repositories/` owns repository names, resolved definitions, target
  selection, and state models.
- `src/git/` owns `GitClient` and the `CommandGitClient` implementation backed
  by `std::process::Command`.
