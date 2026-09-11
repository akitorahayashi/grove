# grove

`grove` manages multiple Git repositories from `grove.toml`. The CLI command is `gv`.

## Quick start

```bash
gv init
```

Register existing repositories with `gv add`, or edit `grove.toml` directly.
See [configuration](docs/config.md) for the schema.

An existing local Git repository can also be registered from the grove root:

```bash
gv add path/to/repository
```

```bash
gv validate
gv sync
gv status
```

## Documentation

- [Usage](docs/usage.md) — commands, status/sync/refresh/clone-cache behavior, requirements, and the library API
- [Configuration](docs/config.md) — `grove.toml` schema, path resolution, includes, and validation rules
- [Architecture](docs/architecture.md) — source layout, module boundaries, and conventions
- [Testing](docs/testing.md) — test layout, verify commands, coverage, and CI
- [Contributing](CONTRIBUTING.md) — development environment and contribution workflow

## Requirements

Git 2.23.0 or newer, on Linux or macOS. See [usage](docs/usage.md#requirements)
for the full requirements and release composition.
