# Configuration

`grove.toml` declares repository names, target paths, and clone URLs.

## Schema

```toml
version = 2

[repos]
dotfiles = "git@github.com:company/dotfiles.git"

[groups.Rust]
frontend = "git@github.com:company/frontend.git"
backend = "git@github.com:company/backend.git"

[groups."Clients/acme"]
service = "git@github.com:acme/service.git"

[groups.Rust.release-tool]
url = "git@github.com:company/release-tool.git"
default_branch = "release/stable"
```

Repositories directly below the directory containing their configuration file
are entries in `repos`. Repositories sharing a parent directory are entries in
`groups.<directory>`. The group key is a normalized relative directory path;
paths containing characters outside TOML bare keys, including `/` and `.`, are
quoted.

A repository whose path follows the group and repository name is a URL string.
The examples resolve to `dotfiles`, `Rust/frontend`, `Rust/backend`,
`Clients/acme/service`, and `Rust/release-tool`, relative to the defining
configuration file. Repository names remain globally unique and are the names
accepted by CLI target arguments. Groups organize configuration and do not form
part of a repository name or become selectable targets.

A repository table carries exceptional fields:

- `url` is required after overrides are merged.
- `path` is optional and replaces the derived path. It remains relative to the
  defining configuration file, not to the group.
- `default_branch` is optional. An explicit branch takes precedence over the
  local `origin/HEAD`; `origin/HEAD` is used when the field is absent. The
  value is a literal branch name under Git's rules, as
  `git check-ref-format --branch` applies them, checked without running Git.
  Non-ASCII characters, Unicode whitespace included, are accepted; `HEAD` and
  `@` are rejected, the latter because `git switch` reads it as `HEAD`.

Root repositories use the same detail form under `repos.<name>`. Repository
declaration order is root entries followed by each group and its entries in
declaration order. Included files follow in include order.

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
writes a minimal version 2 file to the current directory. `gv clone`, `gv cache
list`, and `gv cache clean` without repository names (no targets, or only cache
identities) read no configuration.

## Adding repositories

`gv add` resolves one base configuration from the invocation's current directory
or `--config`, regardless of the repository paths supplied. It adds each existing
Git worktree to that base file in operand order. `--override` changes the
destination to the base file's sibling override; it does not make an override
file independently discoverable or replace a conflicting definition.

Generated names come from canonical worktree directory names. A worktree at the
configuration root becomes a detailed root entry with `path = "."`. Other
worktrees become URL strings in `repos` or in the group named by their relative
parent directory. Generated paths use `/` separators. The origin URL is stored
without a `default_branch`. Username-only `ssh://user@host/...` URLs and
SCP-like SSH URLs are accepted. SSH URLs containing a password, HTTP(S) URLs
containing userinfo, secret query parameters, and relative local URLs are
rejected rather than rewritten.

An existing configured parent directory, including an empty group, keeps its
current group spelling. The group directory itself is compared by resolved
filesystem identity, so case variants and parent-directory symlink aliases do
not cause `gv add` to create another group. A symlinked repository inside an
otherwise different group does not make those parent directories equivalent.
Multiple configured spellings for one resolved parent are ambiguous and stop
the addition.

The destination is edited without reordering or reformatting existing content.
A document whose representation cannot be preserved is rejected with guidance
to add the entry manually. Each prospective edit is validated through the same
base, override, include, and catalog validation as a normal load. The resulting
entry must resolve to the requested worktree and URL before anything is written.
A successful entry atomically replaces the destination and preserves its
permissions; a new override is owner-only and omits `version`. A failure to
synchronize the parent directory after replacement is reported as written with
unconfirmed durability and stops further operands.

## Path resolution

Explicit and derived paths are resolved relative to the configuration file that
defines the repository. Absolute paths and paths that leave the canonical grove
root are rejected. Symlinks are valid when their canonical targets remain inside
the root. Symlink aliases share one operational identity for duplicate and
nested path validation.

## Removed repositories

Leaving a repository out of the configuration never removes it: an entry may be
absent because overrides were ignored, because a machine has no override, or by
mistake. Removal and renaming are declared instead, under `removed`, which
mirrors the live tables.

```toml
version = 2

[groups.Skills]
agent-device = "git@github.com:company/agent-device.git"

[removed.groups.Skills]
agent-device-plugin = { url = "git@github.com:company/agent-device-plugin.git", moved_to = "agent-device" }

[removed.repos]
old-tool = "git@github.com:company/old-tool.git"
```

`removed.repos` and `removed.groups.<directory>` take the same shapes as `repos`
and `groups.<directory>`, and their paths derive by the same rules. A URL string
is shorthand for a table holding only `url`.

| Field | Required | Meaning |
|---|---|---|
| `url` | yes | The old repository's URL; it proves ownership of the old path and names the clone cache entry to drop |
| `path` | no | Replaces the derived old path, as for a live repository |
| `moved_to` | no | The live repository that the old directory becomes |

Removed names are unique among removed entries only. A live repository may
share one, which is how a repository moving between groups is declared:
`[removed.groups.A] tool = { url = "…", moved_to = "tool" }` beside
`[groups.B] tool = "…"`. Removed entries are never CLI targets. Their paths
share the live paths' namespace, so a removed path may neither equal nor nest
with any live or other removed path. A removed path resolves its parent
directories as live paths do but keeps its final component as written, so a
symbolic link there is the path sync assesses, not its target. Each live
repository is the `moved_to` of at most one removed entry. `gv sync` converges declared entries as
described in [usage](usage.md#removed-repositories). A declaration can be
dropped once every machine has synced it.

## Overrides

Loading a configuration file — the root file or an include target — also looks
for a sibling override file named after that file's stem: `grove.toml` pairs
with `grove.override.toml`, and a `--config custom.toml` file pairs with
`custom.override.toml`.

Tables merge key by key, while scalars and arrays — including `include` — are
replaced outright. A base repository URL string, live or removed, is normalized
to its `url` field before merging, so a matching repository table in the
override can change only `path` or `default_branch` (or `moved_to`) without
repeating the URL. Removed entries for repositories that must stay private
belong in the override, next to their live declarations.

```toml
# grove.toml
version = 2

[groups.Rust]
frontend = "git@github.com:company/frontend.git"
```

```toml
# grove.override.toml
[groups.Rust.frontend]
default_branch = "release/stable"
```

An override addresses a grouped repository through the same group and
repository key. A scalar in the override replaces the complete repository
value. A key present only in the override is added; a key present only in the
base file is kept. The override's `version` is optional and, when present,
replaces the base version like any other scalar.

Overrides never affect discovery. An override that exists but cannot be
resolved — malformed TOML, a broken symlink, or a non-file — fails validation.
A schema violation confined to the override is reported against the override
file; a violation that survives the merge is reported against the base file. A
symlinked base configuration pairs with an override beside its resolved target.
`gv sync --ignore-overrides` (`gv sync -i`) instead loads the root and every
included base file without inspecting their sibling overrides. The base
configuration's `include` array is used unchanged, and the resulting base-only
tree must pass the normal validation rules.

## Includes

Root configuration files can include one level of child configuration files.

```toml
version = 2

include = [
  "personal/repositories.toml",
  "work/repositories.toml",
]
```

Child configuration files define repositories relative to their own directory
and cannot include other configuration files. Each base child file declares
`version = 2` and can have its own sibling override. `gv add` guarantees only
the selected root's catalog, so an addition that edits a child file through
another root leaves each including catalog to `gv validate`
([usage](usage.md#add)).

## Validation

`gv validate` loads `grove.toml`, resolves includes, and validates the complete
catalog without inspecting repository working trees or requiring network access.
Configuration version 2 is the only supported version.

Rejected configurations include malformed TOML, unknown fields, and wrong
field types, plus:

- an unsupported or missing `version`
- a missing, empty, or invalid repository name, URL, or branch ref
- an empty, absolute, or non-normalized group directory
- different groups in one merged file that resolve to the same directory
- duplicate repository names or duplicate and nested repository identities
- a removed entry without `url`, with a field other than `url`, `path`, and
  `moved_to`, or with a name repeated among removed entries
- a removed path equal to or nested with a live or another removed path
- a `moved_to` that names no live repository, or a live repository another
  removed entry already moves to
- a `removed` table holding keys other than `repos` and `groups`
- an absolute path, or a path outside the canonical grove root, for a
  repository's `path`
- an absolute, nonexistent, duplicate, or nested include path
- a sibling override file that is malformed TOML, a broken symlink, or not a
  regular file
