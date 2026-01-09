# AAS-ΔSync Task Runner
# Usage: just <target>

# Default target
default: check

# Format all code
fmt:
    cargo fmt --all

# Check formatting
fmt-check:
    cargo fmt --all --check

# Run clippy lints
lint:
    cargo clippy --workspace --all-targets -- -D warnings

# Run all checks (format + lint)
check: fmt-check lint

# Run all tests
test:
    cargo test --workspace

# Run tests with output
test-verbose:
    cargo test --workspace -- --nocapture

# Build all crates
build:
    cargo build --workspace

# Build release binaries
build-release:
    cargo build --workspace --release

# Start Docker Compose demo stack
docker-up:
    cd examples && docker compose up -d

# Stop Docker Compose demo stack
docker-down:
    cd examples && docker compose down -v

# Run the convergence demo
demo: docker-up
    cd examples && ./demo.sh

# Clean build artifacts
clean:
    cargo clean

# Generate protobuf code
proto:
    cargo build -p aas-deltasync-proto --features codegen

# Run integration tests (requires Docker)
integration: docker-up
    cargo test --test integration -- --test-threads=1
    just docker-down

# Full CI pipeline
ci: check test
