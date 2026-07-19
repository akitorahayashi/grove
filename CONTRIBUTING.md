# Contributing

## Contribution Policies

### Coding Standards

- Formatter: `rustfmt` (configuration in `rustfmt.toml`).
- Linter: `clippy` with `-D warnings` (all warnings are errors; configuration in `clippy.toml`).

### Naming Conventions

- Structs and Enums: `PascalCase`
- Functions and Variables: `snake_case`
- Modules: `snake_case`, commands grouped by resource (`sessions/`, `sources/`, `activities/`)

### Adding Tests

- Unit tests: defined in a `#[cfg(test)] mod tests` block in the corresponding source file.
- Command tests: use `FakeJulesApi` from `src/testing/fake_julius_api.rs` to stub port responses.
- HTTP client tests: use `mockito::Server` to mock HTTP responses.
- CLI tests: located in `tests/cli/` and registered in `tests/cli/mod.rs`.
- Library tests: located in `tests/library/commands/` and registered in `tests/library/commands/mod.rs`.

### Configuration Files

| File | Purpose |
|------|---------|
| `rust-toolchain.toml` | Pinned Rust toolchain version |
| `Cargo.toml` | Dependencies and package metadata |
| `rustfmt.toml` | Formatter configuration |
| `clippy.toml` | Linter configuration |

## Procedural Verification

### Verify Commands

All commands are run before submitting changes:

```bash
just check
just test
```
