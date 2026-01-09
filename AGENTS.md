# Repository Guidelines

## Project Structure & Module Organization
- `crates/`: Rust workspace crates. Core logic lives in `aas-deltasync-core`, protocol definitions in `aas-deltasync-proto`, adapters in `aas-deltasync-adapter-*`, runtime in `aas-deltasync-agent`, and CLI in `aas-deltasync-cli`.
- `proto/`: Protobuf sources and related artifacts for the wire protocol.
- `examples/`: Docker Compose demo stack and scripts (see `examples/demo.sh`).
- `docs/`: Design notes and architecture references (e.g., `docs/design/crdt-mapping.md`).
- `specs/`: Reference material used during development.
- Workspace root: `Cargo.toml`, `Cargo.lock`, and `justfile` for task automation.

## Build, Test, and Development Commands
- `just check`: Run formatting and lint checks (`cargo fmt --check` + `cargo clippy`).
- `just test`: Run all workspace tests (`cargo test --workspace`).
- `just test-verbose`: Run tests with output (`--nocapture`).
- `just build` / `just build-release`: Build debug or release binaries.
- `just demo`: Start the Docker demo stack and run the convergence demo.
- `just integration`: Run integration tests (requires Docker).
- Direct equivalents are available via `cargo` (see `justfile`).

## Coding Style & Naming Conventions
- Rust formatting is enforced by `rustfmt`; run `just fmt` before committing.
- Lint with `clippy` via `just lint` (warnings are denied).
- Follow Rust naming conventions: `snake_case` for modules/functions, `PascalCase` for types/traits, `SCREAMING_SNAKE_CASE` for constants.
- Public APIs should include rustdoc comments.

## Testing Guidelines
- Unit tests live alongside crate modules under `crates/*/src` using `#[cfg(test)]`.
- Integration tests are expected under `tests/` when added; `just integration` runs the Docker-backed suite.
- Keep tests deterministic and runnable via `cargo test --workspace`.

## Commit & Pull Request Guidelines
- Git history is not available in this checkout; follow Conventional Commits per `CONTRIBUTING.md` (e.g., `feat:`, `fix:`, `docs:`, `test:`, `refactor:`, `chore:`).
- Branch from `main`, add tests for new behavior, and ensure `just ci` passes.
- Use the PR template and include clear descriptions and any relevant logs or screenshots.

## Security & Configuration Tips
- Review `SECURITY.md` before reporting vulnerabilities.
- Configuration examples live in `README.md` and `examples/`; avoid committing secrets or private keys.
