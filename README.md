# rs-cli-tmpl

`rs-cli-tmpl` is a reference template for building Rust CLI tools with
concept-owned module boundaries. It demonstrates growth from one concept
(`items`) to two sibling concept owners (`items`, `labels`) and one
cross-concept orchestration family (`app/labeling`).

## Architectural Highlights

- Top-level ownership is explicit:
  - `src/cli/` owns command-line parsing and terminal output.
  - `src/app/` owns use-case orchestration.
  - `src/items/` owns item rules and storage.
  - `src/labels/` owns label rules and storage.
  - `src/error.rs` owns application-wide errors.
- Concept contracts and implementations are co-located:
  - `src/items/store.rs` + `src/items/storage/filesystem_store.rs`
  - `src/labels/store.rs` + `src/labels/storage/filesystem_store.rs`
- Cross-concept behavior is isolated in `src/app/labeling/` instead of leaking
  into either concept module.

## Quick Start

```bash
cargo install --path .
# or
cargo build --release
```

The optimized binary is created at `target/release/rs-cli-tmpl`.

## Usage

```bash
rs-cli-tmpl --version

rs-cli-tmpl item add <item-id> --content "..."
rs-cli-tmpl item list
rs-cli-tmpl item delete <item-id>

rs-cli-tmpl label add <label-name>
rs-cli-tmpl label list
rs-cli-tmpl label delete <label-name>

rs-cli-tmpl labeling attach <item-id> <label-name>
rs-cli-tmpl labeling detach <item-id> <label-name>
rs-cli-tmpl labeling list <item-id>
rs-cli-tmpl labeling find --label <label-name>
```

## Storage Layout

The template stores concept data under `~/.config/rs-cli-tmpl/`:

```text
~/.config/rs-cli-tmpl/
  items/
    <item-id>/
      item.txt
  labels/
    definitions/
      <label-name>/
        label.txt
    links/
      <item-id>/
        <label-name>
```

## Development Commands

- `just setup`: install pinned development tools from `mise.toml`.
- `cargo build`: build a debug binary.
- `cargo build --release`: build the optimized release binary.
- `cargo fmt`: format code using rustfmt.
- `cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings`.
- `cargo test --all-targets --all-features`.
- `just coverage`: run coverage with pinned tarpaulin.
- `cargo fetch --locked`: pre-fetch dependencies.

## Testing Culture

- Unit tests live beside each owner module in `src/`.
- Command logic tests use concept-local test doubles:
  - `src/items/testing.rs`
  - `src/labels/testing.rs`
- Integration tests live in `tests/`:
  - `tests/cli.rs` verifies the CLI boundary.
  - `tests/library.rs` verifies the public library boundary.
  - `tests/harness/test_context.rs` provides shared fixtures.

## Project Structure

```text
rs-cli-tmpl/
├── src/
│   ├── main.rs
│   ├── lib.rs
│   ├── error.rs
│   ├── cli/
│   │   ├── mod.rs
│   │   ├── item.rs
│   │   ├── label.rs
│   │   └── labeling.rs
│   ├── app/
│   │   ├── api.rs
│   │   ├── context.rs
│   │   ├── items/
│   │   ├── labels/
│   │   └── labeling/
│   ├── items/
│   │   ├── item_id.rs
│   │   ├── store.rs
│   │   ├── testing.rs
│   │   └── storage/
│   └── labels/
│       ├── label_name.rs
│       ├── store.rs
│       ├── testing.rs
│       └── storage/
├── tests/
│   ├── cli.rs
│   ├── library.rs
│   ├── cli/
│   ├── library/
│   └── harness/
└── docs/
    └── architecture.md
```

## Adapting the Template

1. Replace the sample use-case modules under `src/app/items/`,
   `src/app/labels/`, and `src/app/labeling/`.
2. Extend `src/app/api.rs` to expose your library-facing use cases.
3. Update `src/cli/` to match your command surface.
4. Add new concept owners as siblings of `src/items/` and `src/labels/`.
5. Keep integration tests and architecture docs aligned with the implementation.
