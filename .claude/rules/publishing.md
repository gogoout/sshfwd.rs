# Publishing to crates.io

**CRITICAL:** Never run `cargo publish` from local machine. Publishing is fully automated via GitHub Actions.

## Release Process

1. **Update version** in root `Cargo.toml`:
   - `[workspace.package]` version = "X.Y.Z"
   - `[workspace.dependencies]` sshfwd-common version = "X.Y.Z"

2. **Commit and tag:**
   ```bash
   git add Cargo.toml
   git commit -m "Bump version to X.Y.Z"
   git tag vX.Y.Z
   git push origin vX.Y.Z
   ```

3. **GitHub Actions workflow automatically:**
   - Builds agent binaries for all 4 platforms (Linux x86_64/ARM64, macOS Intel/ARM64)
   - Publishes `sshfwd-common@X.Y.Z` to crates.io
   - Waits 60 seconds for crates.io index update
   - Publishes `sshfwd@X.Y.Z` with embedded agent binaries

## Requirements

- `CARGO_REGISTRY_TOKEN` secret must be configured in GitHub repository settings
- Agent binaries must exist in `crates/sshfwd/prebuilt-agents/` (committed to repo)
- All CI checks must pass before tagging

## Workflow File

See `.github/workflows/release.yml` for the automated release implementation.
