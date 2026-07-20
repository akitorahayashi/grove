# Contributing

## Development environment

The repository toolchain is declared in `rust-toolchain.toml`. Cargo uses Rust
1.90.0 with the `rustfmt` and `clippy` components. Development tools are pinned
by `mise.toml` and `mise.lock`.

`just setup` installs the locked mise tools. The normal verification sequence
is:

```bash
just fix
just check
just test
```

`just coverage` runs tarpaulin with the repository's 30 percent floor.

## Source ownership

- `src/cli/` owns argument adaptation, terminal rendering, and output errors.
- `src/app/` owns init, status, sync, and validation use-case orchestration.
- `src/config/` owns discovery, include loading, TOML decoding, and catalog
  validation.
- `src/repositories/` owns validated names, branch names, URLs, operational
  paths, definitions, and selection.
- `src/git/` and `src/zoxide/` own their respective process boundaries.
- `src/error.rs` owns application-wide errors.

The supported library API is re-exported from `src/lib.rs`. Internal process
clients, dependency traits, workers, events, and owner modules are not public
compatibility surfaces.

## Tests

Unit tests live beside their owning modules. CLI contract tests live under
`tests/cli/`, public facade tests live under `tests/library/`, and local Git
fixtures live under `tests/harness/`.

Filesystem and process tests use temporary directories. Concurrency tests use
synchronization primitives rather than elapsed-time assumptions.

## Automation

CI runs format, lint, test, coverage, debug build, and release build stages with
the repository Rust toolchain. Tests run on Linux and macOS. External GitHub
Actions are pinned to reviewed commit identifiers with version comments; action
updates review both the commit and its release notes.
