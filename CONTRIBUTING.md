# Contributing to AAS-ΔSync

Thank you for your interest in contributing! This document provides guidelines and instructions.

## Code of Conduct

This project adheres to the [Contributor Covenant Code of Conduct](CODE_OF_CONDUCT.md).

## How to Contribute

### Reporting Issues

- Check existing issues before creating a new one
- Use the issue templates provided
- Include reproduction steps and environment details

### Pull Requests

1. **Fork** the repository
2. **Create a branch** from `main` for your changes
3. **Write tests** for new functionality
4. **Ensure CI passes**: `just ci`
5. **Submit a PR** using the template

### Development Setup

```bash
# Clone your fork
git clone https://github.com/YOUR_USERNAME/aas-deltasync
cd aas-deltasync

# Install Rust (if needed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install just (task runner)
cargo install just

# Run checks
just check
just test
```

### Code Style

- Run `just fmt` before committing
- Run `just lint` to check for issues
- Follow Rust API guidelines
- Document public APIs with rustdoc

### Commit Messages

Use conventional commits:
- `feat:` New features
- `fix:` Bug fixes
- `docs:` Documentation changes
- `test:` Test additions/changes
- `refactor:` Code refactoring
- `chore:` Maintenance tasks

### Testing

- Unit tests in each crate
- Integration tests in `tests/` directory
- Run `just integration` for full stack tests (requires Docker)

## Architecture Decisions

Major changes should be discussed in an issue before implementation. Consider:

- AAS standard compliance
- CRDT consistency guarantees
- Performance implications
- Backward compatibility

## License

By contributing, you agree that your contributions will be licensed under Apache-2.0.
