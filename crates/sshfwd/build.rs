use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// Platform targets for embedded agent binaries.
const PLATFORMS: &[(&str, &str, &str)] = &[
    // (os, arch, directory name)
    ("linux", "x86_64", "linux-x86_64"),
    ("linux", "aarch64", "linux-aarch64"),
    ("darwin", "x86_64", "darwin-x86_64"),
    ("darwin", "aarch64", "darwin-aarch64"),
];

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let prebuilt_dir = manifest_dir.join("../../prebuilt-agents");

    let mut code = String::new();
    code.push_str("/// Returns the embedded agent binary for the given OS and architecture.\n");
    code.push_str("/// Returns `None` if no binary was available at build time.\n");
    code.push_str("pub fn get_agent_binary(os: &str, arch: &str) -> Option<&'static [u8]> {\n");
    code.push_str("    match (os, arch) {\n");

    for &(os, arch, dir_name) in PLATFORMS {
        let binary_path = prebuilt_dir.join(dir_name).join("sshfwd-agent");
        let canonical = binary_path.display().to_string();

        // Tell cargo to re-run if the binary changes
        println!("cargo:rerun-if-changed={canonical}");

        if binary_path.exists() {
            code.push_str(&format!(
                "        (\"{os}\", \"{arch}\") => Some(include_bytes!(\"{canonical}\")),\n"
            ));
        } else {
            code.push_str(&format!(
                "        // not found at build time: {canonical}\n"
            ));
            code.push_str(&format!("        (\"{os}\", \"{arch}\") => None,\n"));
        }
    }

    code.push_str("        _ => None,\n");
    code.push_str("    }\n");
    code.push_str("}\n");

    let dest = out_dir.join("embedded_agents.rs");
    fs::write(&dest, code).expect("failed to write embedded_agents.rs");

    // Re-run if the prebuilt-agents directory structure changes
    rerun_if_dir_changed(&prebuilt_dir);
}

fn rerun_if_dir_changed(dir: &Path) {
    println!("cargo:rerun-if-changed={}", dir.display());
    if dir.is_dir() {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    println!("cargo:rerun-if-changed={}", path.display());
                }
            }
        }
    }
}
