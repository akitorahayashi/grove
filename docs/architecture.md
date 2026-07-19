# Architecture

## Intent

`rs-cli-tmpl` is a reference template for Rust command line tools. The current
template demonstrates a concept-owned boundary model with two sample concepts,
`items` and `labels`, plus one cross-concept orchestration family,
`app/labeling`. The sample concepts are illustrative. The architectural point
is that top-level modules identify owners, while internal submodules express
specific boundary roles.

This document describes the current implementation and the design intent it
expresses.

## Design axis

The template is organized by owning concept at the top level.

- `src/cli` owns command-line parsing and terminal output
- `src/app` owns dependency wiring and use-case orchestration
- `src/items` owns the item concept end-to-end
- `src/labels` owns the label concept end-to-end
- `src/error.rs` owns the application-wide error type

Boundary roles such as contract, validation, and storage implementation are
kept inside their owning concept modules instead of being promoted to top-level
taxonomies.

## Current structure

```text
src/
  error.rs
  cli/
    mod.rs
    item.rs
    label.rs
    labeling.rs
  app/
    api.rs
    context.rs
    items/
      add.rs
      list.rs
      delete.rs
    labels/
      add.rs
      list.rs
      delete.rs
    labeling/
      attach.rs
      detach.rs
      list.rs
      find.rs
  items/
    item_id.rs
    store.rs
    storage/
      filesystem_store.rs
      settings.rs
    testing.rs
  labels/
    label_name.rs
    store.rs
    storage/
      filesystem_store.rs
      settings.rs
    testing.rs
  lib.rs
  main.rs
```

`src/lib.rs` is the public library surface and CLI entrypoint export.
`src/main.rs` is the binary entrypoint that delegates to the library.

## Ownership rules

### cli/

`src/cli` defines the command surface (`item`, `label`, `labeling`) and maps
CLI interactions to application API calls. It does not own use-case logic,
concept invariants, or persistence rules.

### app/

`src/app` coordinates use cases and dependency wiring.
It does not own concept invariants or storage layout.

Current examples:

- `api.rs` provides library-facing orchestration
- `context.rs` carries injected dependencies for both stores
- `items/` contains item-only use cases
- `labels/` contains label-only use cases
- `labeling/` contains cross-concept use cases

### items/

`src/items` owns the sample item concept, including validation, dependency
contract, concrete storage, and concept-specific test support.

Current example:

- `item_id.rs` validates the `ItemId` invariant
- `store.rs` defines `ItemStore`
- `storage/filesystem_store.rs` implements `ItemStore` with filesystem I/O
- `storage/settings.rs` owns storage configuration used by that implementation
- `testing.rs` provides `MockItemStore` for unit tests

### labels/

`src/labels` owns the sample label concept, including validation, dependency
contract, concrete storage, and concept-specific test support.

Current example:

- `label_name.rs` validates the `LabelName` invariant
- `store.rs` defines `LabelStore`
- `storage/filesystem_store.rs` implements `LabelStore` with filesystem I/O
- `storage/settings.rs` owns storage configuration used by that implementation
- `testing.rs` provides `MockLabelStore` for unit tests

### error.rs

`src/error.rs` owns `AppError` when error semantics are application-wide.

## Dependency direction

Dependency flow remains inward toward concept contracts and validation.

```text
main -> lib -> cli -> app
app::api -> items::storage + labels::storage for default wiring
app::items -> items::store + items::item_id
app::labels -> labels::store + labels::label_name
app::labeling -> items::store + items::item_id + labels::store + labels::label_name
items::storage -> items::store + items::item_id
labels::storage -> labels::store + labels::label_name
lib -> cli + app + items + labels + error
```

Concept modules do not depend on `app` or CLI parsing.

## Sample scope

The sample remains intentionally small while showing multi-owner growth.

- `ItemId` demonstrates a pure validated type
- `ItemStore` demonstrates a dependency contract
- `FilesystemItemStore` demonstrates an adapter implementation
- `LabelName` demonstrates a second validated type under a sibling owner
- `LabelStore` demonstrates a second concept contract
- `app/labeling` demonstrates cross-concept orchestration

The template therefore teaches how concepts own internal boundaries without
introducing top-level `ports` or `adapters` buckets.

## Growth path

As a project grows, new concepts are added as new top-level owners beside
`items` and `labels`.

```text
src/
  cli/
  app/
  items/
  labels/
  github/
  exchange/
  workflow/
```

Each concept can then define its own validated types, contracts,
implementations, and test support internally.

## Testing model

Tests follow the same explicit boundaries.

- unit tests live next to the modules they verify
- `src/items/testing.rs` and `src/labels/testing.rs` provide concept-local test
  doubles for command logic
- `tests/cli.rs` verifies the CLI boundary
- `tests/library.rs` verifies the public library boundary
- `tests/harness/` provides shared integration fixtures
