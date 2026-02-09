# Workspace Dependency Management

- All dependency **versions** are defined in root `Cargo.toml` under `[workspace.dependencies]`
- **Features** are NOT specified in the root — declare them in each crate's `Cargo.toml`
- Sub-crates reference: `dep = { workspace = true, features = ["..."] }`

This keeps versions centralized while making feature usage explicit per crate.
