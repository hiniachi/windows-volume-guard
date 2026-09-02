# Contributing

Thank you for contributing to Windows Volume Guard.

## Development

Development and testing require Windows 10 or 11 and the stable Rust toolchain.

```powershell
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```

## Pull requests

1. Create a focused branch from `main`.
2. Keep each pull request limited to one change.
3. Add or update tests and documentation when behavior changes.
4. Confirm that all CI checks pass.

Security vulnerabilities must be reported privately as described in [SECURITY.md](SECURITY.md).
