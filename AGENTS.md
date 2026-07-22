# grove Development Overview

## Project Summary

`grove` is a Rust CLI for managing multiple Git repositories from
`grove.toml`. The CLI command is `gv`. It clones missing repositories, reports
repository state, and safely fast-forwards existing repositories' default
branches through the system `git` command. Refresh operations leave successful
working trees on their default branches.

## Documentation

- [Architecture](docs/architecture.md) — source layout, module boundaries, naming conventions, data flow
- [Configuration](docs/config.md) — `grove.toml` schema, path resolution, includes, validation rules
- [Usage](docs/usage.md) — CLI command behavior and the library API
- [Testing](docs/testing.md) — test layout, coverage, CI

Top-level owners: `cli/` for interface adaptation, `app/` for use-case
orchestration, `cache/` for the local clone cache, `phases/` for phase-
structured parallel execution, `inspection.rs` for repository readiness
diagnosis, `config/` for `grove.toml`, `repositories/` for managed repository
domain data, `git/` for the system Git boundary, and `error.rs` for
application-wide errors.

## Verify Commands

```bash
just fix
just check
just test
```

Run `fix` before `check`; `check` does not modify files. See
[testing](docs/testing.md) for what each recipe wraps, coverage, and CI.
