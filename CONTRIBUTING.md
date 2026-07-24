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

`just coverage` runs tarpaulin with the repository's 86 percent floor.

## Source ownership

See [architecture](docs/architecture.md) for module boundaries and the
supported library API surface.

## Tests

See [testing](docs/testing.md) for test layout and conventions.

## Automation

Third-party GitHub Actions are pinned to reviewed commit identifiers with
version comments. Actions owned by `akitorahayashi` use reviewed release or
major tags instead of commit pins. Action updates review both the pinned
identifier and its release notes. Write permissions are scoped to the release
jobs that need them; other jobs default to read-only. CI stages are
documented in [testing](docs/testing.md).
