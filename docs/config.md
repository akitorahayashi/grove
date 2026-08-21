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

## Discovery

Commands that read configuration locate `grove.toml` by searching the current
directory and then its ancestors, so they work from anywhere inside the tree the
root file covers. The nearest file wins, which leaves a nested `grove.toml`
authoritative for its own subtree. A `grove.toml` entry that exists but cannot
be resolved — a broken symlink, an unreadable file, or a non-file — ends the
search as a failure rather than deferring to a parent. When the search ascends
above the current directory, the resolved file is named on stderr; `gv
validate` is the exception, always naming the file on stdout in its summary.

`--config <path>` addresses a file directly and performs no search. `gv init`
writes to the current directory, and `gv clone`, `gv cache list`, and
`gv cache clean` without repository names read no configuration.

## Path resolution

Explicit paths are resolved relative to the `grove.toml` file that defines
the repository. Absolute paths and paths that leave the canonical grove root
are rejected. Symlinks are valid when their canonical targets remain inside
the root. Symlink aliases share one operational identity for duplicate and
nested path validation.

## Overrides

Loading a configuration file — the root file or an include target — also looks
for a sibling override file named after that file's stem: `grove.toml` pairs
with `grove.override.toml`, and a `--config custom.toml` file pairs with
`custom.override.toml`. When the sibling exists, it is deep merged into the
file it overrides before schema decoding: tables merge key by key, recursing
into nested tables, while scalars and arrays — including `include` — are
replaced outright rather than concatenated. A key present only in the override
is added; a key present only in the base file is kept. The override's
`version` is optional; when present it replaces the base file's `version` like
any other scalar, so an unsupported override `version` fails validation the
same way an unsupported base `version` does.

Overrides never affect discovery: a `grove.override.toml` without a sibling
`grove.toml` is not a grove root. An override that exists but cannot be
resolved — malformed TOML, a broken symlink, or a non-file — fails validation
instead of being silently ignored. A schema violation confined to the
override itself (an unknown field, a wrong type) is reported against the
override file; a violation that survives the merge — including one only
reachable through the base file's own value — is reported against the base
file, consistent with [path resolution](#path-resolution) and includes
treating the base file's canonicalized location as the pair's identity. A
symlinked `grove.toml` therefore pairs with an override beside its resolved
target, not beside the symlink.

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

Rejected configurations include malformed TOML, unknown fields, and wrong
field types, plus:

- an unsupported or missing `version`
- a missing, empty, or invalid repository name, URL, or branch ref
- duplicate or nested repository identities
- an absolute path, or a path outside the canonical grove root, for a
  repository's `path`
- an absolute, nonexistent, duplicate, or nested include path
- a sibling override file that is malformed TOML, a broken symlink, or not a
  regular file
