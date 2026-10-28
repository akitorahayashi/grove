# Testing

## Unit tests

Unit tests live beside the modules they verify.

## Integration tests

Integration tests live in `tests/`, split into two Cargo integration test
binaries plus a shared fixture module:

- `tests/cli.rs` is the crate root for CLI boundary behavior. It pulls in
`tests/cli/`, one file per subcommand: `add.rs`, `cache.rs`, `clone.rs`, `init.rs`,
  `status.rs`, and `validate.rs`, plus responsibility-oriented `refresh/` and
  `sync/` modules. `refresh/` splits out `planning` and `safety`; `sync/`
  splits out `planning`, `cache`, `progress`, `removal`, and `zoxide`. Each `mod.rs`
  keeps the module wiring and its subcommand's core flow.
- `tests/library.rs` is the crate root for public library-facade behavior. It
  pulls in `tests/library/`: `error_contract.rs`, `facade.rs`,
  `unknown_repository_fails.rs`, and `validate.rs`.
- `tests/harness/test_context.rs` provides `TestContext`, shared by both
  binaries: a temporary workspace and cache home, a `gv` command pre-wired to
  them, and Git remote/seed repository setup through `run_git`.
- `tests/harness/git_process.rs` owns Git process isolation and the shared
  local-commit and Git-wrapper fixtures used by failure-path suites.

Filesystem and process tests use temporary directories. Nothing reaches the
real trash: unit tests inject a trash double, and the CLI test that trashes a
repository runs on Linux only, with `XDG_DATA_HOME` inside its temporary
directory. Git observation parsers
are tested against independent machine-readable fixtures. Concurrency tests
use a deadline-bounded rendezvous rather than elapsed-time assumptions, so a
parallelism regression fails instead of hanging, and they assert the externally
visible ordering rather than Git invocation counts.

## Git process isolation

Fixture Git processes and `gv` children run without any inherited `GIT_*`
variable and with `GIT_CONFIG_GLOBAL=/dev/null` and `GIT_CONFIG_NOSYSTEM=1`,
so a developer's signing, hooks, aliases, or an ambient `GIT_DIR` neither
break fixture setup nor redirect it. Global-configuration isolation relies on
`GIT_CONFIG_GLOBAL`, which Git 2.32 introduced. Isolation is applied per child;
the test process environment is never mutated. A test sets its own variables
after the harness, so tests of the production environment policy inject
repository selectors deliberately.

| Owner | Isolated processes |
| --- | --- |
| `tests/harness/git_process.rs` | `git_command`, `run_git`, `git_stdout`, and the `gv` children from `TestContext::cli` and `TestContext::gv_process` |
| `src/git/command.rs` tests | Unit fixture Git processes and `CommandGitClient::isolated` clients |

Library integration tests run grove in the test process, so the Git processes
grove spawns there inherit the test process environment.

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
`coverage/`, with an 86 percent floor.

## Continuous integration

CI runs four reusable workflows orchestrated by `ci-workflows.yml`:

- Static checks — `just check` on Ubuntu.
- Build — a debug build, plus release binaries for `x86_64`/`aarch64` on
  Linux and macOS.
- Test — `just test` on an Ubuntu and macOS matrix.
- Coverage — `just coverage` on Ubuntu only, with `RUST_TEST_THREADS=1`.

GitHub Actions pinning policy is documented in
[Contributing](../CONTRIBUTING.md).

Tag releases first verify that the `v` tag matches the single workspace package
version reported by Cargo metadata. The reusable static-check and Linux/macOS
test workflows must then pass before release preparation, artifact upload, and
publication become reachable.
