# Default: list recipes
default:
    @just --list

# Install all tools to ~/.cargo/bin
install:
    for crate in crates/*/; do cargo install --path "$crate" --root ~/.cargo; done

# Uninstall all tools
uninstall:
    for crate in crates/*/; do basename "$crate" | xargs cargo uninstall --root ~/.cargo; done

# Build all tools (dev)
build:
    cargo build --workspace

# Run all tests
test:
    cargo test --workspace

# Lint
lint:
    cargo clippy --workspace

# Format
fmt:
    cargo fmt --all
