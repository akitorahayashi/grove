# grove

`grove` manages multiple Git repositories from `grove.toml`. The CLI command is `gv`.

## Usage

```bash
gv --version

gv status
gv status frontend
gv status --fetch
gv validate

gv sync
gv sync frontend
gv sync -z
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

`gv validate` loads `grove.toml`, resolves includes, and validates configuration
without inspecting repository working trees or requiring network access.

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

`gv sync --register-zoxide` and its short form `gv sync -z` register synced
repositories with zoxide. Skipped and blocked repositories are not registered,
and dry runs only report the repositories that would be registered.
