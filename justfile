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

# Audit dependencies for security vulnerabilities
audit:
    @echo "📦 Dependency audit:"
    @echo ""
    @echo "Direct dependencies:"
    cargo tree --depth 1
    @echo ""
    @echo "For more advanced checks:"
    @echo "  - cargo install cargo-outdated && cargo outdated"
    @echo "  - cargo install cargo-deny && cargo deny check"
