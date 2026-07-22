# Testing

## Unit tests

Unit tests live beside the modules they verify.

## Integration tests

Integration tests live in `tests/`, split into two Cargo integration test
binaries plus a shared fixture module:

- `tests/cli.rs` is the crate root for CLI boundary behavior. It pulls in
  `tests/cli/`, one file per subcommand: `cache.rs`, `clone.rs`, `init.rs`,
  `refresh.rs`, `status.rs`, `sync.rs`, `validate.rs`.
- `tests/library.rs` is the crate root for public library-facade behavior. It
  pulls in `tests/library/`: `facade.rs`, `unknown_repository_fails.rs`,
  `validate.rs`.
- `tests/harness/test_context.rs` provides `TestContext`, shared by both
  binaries: a temporary workspace and cache home, a `gv` command pre-wired to
  them, and Git remote/seed repository setup through `run_git`.

Filesystem and process tests use temporary directories. Concurrency tests use
synchronization primitives rather than elapsed-time assumptions.

## Verify commands

```bash
just fix
just check
just test
```

`just fix` runs `cargo fmt` and `just --fmt`. `just check` runs `cargo fmt
--check`, `cargo clippy --all-targets --all-features -- -D warnings`, and
`just --fmt --check`. `just test` runs `cargo test --all-targets
--all-features`. Run `fix` before `check`; `check` does not modify files.

## Coverage

```bash
just coverage
```

Runs `cargo tarpaulin` with the llvm engine against a dedicated
`target/tarpaulin` directory, producing stdout and HTML reports under
`coverage/`, with a 30 percent floor.

## Continuous integration

CI runs four reusable workflows orchestrated by `ci-workflows.yml`:

- Static checks — `just check` on Ubuntu.
- Build — a debug build, plus release binaries for `x86_64`/`aarch64` on
  Linux and macOS.
- Test — `just test` on an Ubuntu and macOS matrix.
- Coverage — `just coverage` on Ubuntu only, with `RUST_TEST_THREADS=1`.

External GitHub Actions are pinned to reviewed commit identifiers; see
[Contributing](../CONTRIBUTING.md) for the update policy.
