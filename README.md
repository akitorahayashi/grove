# grove

`grove` manages multiple Git repositories from `grove.toml`. The CLI command is
`gv`.

## Usage

```bash
gv --version

gv status
gv status frontend
gv status --fetch

gv sync
gv sync frontend
gv sync --dry-run
gv --config ~/workspace/grove.toml status
```

## Configuration

`grove.toml` declares repository names, target paths, and clone URLs.

```toml
version = 1

[[repo]]
name = "frontend"
path = "apps/frontend"
url = "git@github.com:company/frontend.git"

[[repo]]
name = "backend"
path = "services/backend"
url = "git@github.com:company/backend.git"
```

The `path` value is resolved relative to the `grove.toml` file that defines the
repository. Absolute repository paths are rejected.

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

## Sync Behavior

`gv sync` clones missing repositories and safely updates existing repositories'
default branches. Existing repositories are updated through system `git`
commands.

The update flow is:

```text
git fetch origin --prune
git switch <default-branch>
git merge --ff-only origin/<default-branch>
git switch <original-branch>
```

The following operations are never performed automatically:

- `git stash`
- `git reset --hard`
- `git rebase`
- `git clean`
- forced checkout
- forced push

## Project Structure

```text
src/
  cli/
  app/
  config/
  repositories/
  git/
  error.rs
  lib.rs
  main.rs
```

- `src/cli/` owns command-line parsing and terminal output.
- `src/app/` owns use-case orchestration and dependency wiring.
- `src/config/` owns `grove.toml` discovery, include resolution, path
  normalization, and validation.
- `src/repositories/` owns repository names, definitions, and target selection.
- `src/git/` owns the system `git` command boundary.
- `src/error.rs` owns application-wide errors.

## Development Commands

- `just setup`: install pinned development tools from `mise.toml`.
- `cargo build`: build a debug binary.
- `cargo build --release`: build an optimized binary.
- `just fix`: format Rust and justfile sources.
- `just check`: verify formatting and linting.
- `cargo test --all-targets --all-features`: run tests.
- `just coverage`: run coverage with pinned tarpaulin.
