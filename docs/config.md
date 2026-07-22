# Configuration

`grove.toml` declares repository names, target paths, and clone URLs.

## Schema

```toml
version = 1

[repos.frontend]
url = "git@github.com:company/frontend.git"
default_branch = "main"

[repos.backend]
path = "services/backend"
url = "git@github.com:company/backend.git"

[repos."company.service"]
url = "git@github.com:company/service.git"
```

Repository names are the direct table keys under `repos`. Names containing
`.` use quoted table keys. The `path` value is optional and defaults to the
repository name.

`default_branch` is optional. An explicitly configured branch takes
precedence over `origin/HEAD`; `origin/HEAD` is used only when the field is
absent. Branch names are validated as Git refs without invoking Git, so `gv
validate` remains independent of installed external tools.

## Path resolution

Explicit paths are resolved relative to the `grove.toml` file that defines
the repository. Absolute paths and paths that leave the canonical grove root
are rejected. Symlinks are valid when their canonical targets remain inside
the root. Symlink aliases share one operational identity for duplicate and
nested path validation.

## Includes

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

## Validation

`gv validate` loads `grove.toml`, resolves includes, and validates the
complete catalog without inspecting repository working trees or requiring
network access. See [usage](usage.md) for the command's CLI invocation.

A `grove.toml` is rejected when it:

- declares an unsupported `version`
- declares duplicate or nested includes
- contains an invalid repository name or branch ref
- declares duplicate or nested repository identities
- uses an absolute path, or a path outside the canonical grove root, for a
  repository's `path`
