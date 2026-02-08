/// Returns the embedded agent binary for the given OS and architecture.
/// Returns `None` if no binary was available at build time.
pub fn get_agent_binary(os: &str, arch: &str) -> Option<&'static [u8]> {
    match (os, arch) {
        ("linux", "x86_64") => Some(include_bytes!("/Users/gogoout/IdeaProjects/sshfwd.rs/crates/sshfwd/../../prebuilt-agents/linux-x86_64/sshfwd-agent")),
        ("linux", "aarch64") => Some(include_bytes!("/Users/gogoout/IdeaProjects/sshfwd.rs/crates/sshfwd/../../prebuilt-agents/linux-aarch64/sshfwd-agent")),
        ("darwin", "x86_64") => Some(include_bytes!("/Users/gogoout/IdeaProjects/sshfwd.rs/crates/sshfwd/../../prebuilt-agents/darwin-x86_64/sshfwd-agent")),
        ("darwin", "aarch64") => Some(include_bytes!("/Users/gogoout/IdeaProjects/sshfwd.rs/crates/sshfwd/../../prebuilt-agents/darwin-aarch64/sshfwd-agent")),
        _ => None,
    }
}
