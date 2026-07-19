# ==============================================================================
# justfile for grove development
# ==============================================================================

set shell := ["bash", "-eu", "-o", "pipefail", "-c"]

# Show available recipes
default: help

# Show available recipes
help:
    @echo "Usage: just [recipe]"
    @echo ""
    @echo "Development tasks for grove:"
    @mise exec -- just --list | tail -n +2 | awk '{printf "  \033[36m%-20s\033[0m %s\n", $1, substr($0, index($0, $2))}'

# ==============================================================================
# Environment Setup
# ==============================================================================

# Initialize project: install dependencies
setup:
    @echo "Installing tools with mise..."
    @mise trust
    @mise install --locked

# ==============================================================================
# Lint & Format
# ==============================================================================

# Format code
fix:
    cargo fmt
    mise exec -- just --fmt --unstable

# Verify formatting, lint, and compilation
check:
    cargo fmt --check
    cargo clippy --all-targets --all-features -- -D warnings
    mise exec -- just --fmt --check --unstable

# ==============================================================================
# Testing
# ==============================================================================

# Run all tests
test:
    cargo test --all-targets --all-features --quiet

# Generate code coverage report
coverage:
    rm -rf target/tarpaulin coverage
    mise exec -- cargo tarpaulin \
        --engine llvm \
        --target-dir target/tarpaulin \
        --packages grove \
        --out Stdout \
        --out Html \
        --output-dir coverage \
        --all-features \
        --fail-under 30

# ==============================================================================
# Build Tasks
# ==============================================================================

# Compile the project
build:
    cargo build
