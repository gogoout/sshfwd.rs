use std::borrow::Cow;
use std::path::Path;

use russh::client::Msg;
use russh::ChannelStream;
use sha2::{Digest, Sha256};

use crate::error::SshError;
use crate::ssh::session::Session;

const REMOTE_AGENT_DIR: &str = ".sshfwd";
const REMOTE_AGENT_NAME: &str = "sshfwd-agent";
const REMOTE_PID_FILE: &str = ".sshfwd/agent.pid";

/// Detected remote platform (OS + architecture).
#[derive(Debug, Clone)]
pub struct Platform {
    pub os: String,
    pub arch: String,
}

impl Platform {
    /// Returns the target triple directory name, e.g. "x86_64-unknown-linux-musl".
    #[allow(dead_code)] // Reserved for future use in prebuilt-agents fallback path
    pub fn target_triple(&self) -> String {
        let vendor_os = match self.os.as_str() {
            "linux" => "unknown-linux-musl",
            "darwin" => "apple-darwin",
            _ => "unknown-unknown",
        };
        format!("{}-{vendor_os}", self.arch)
    }
}

/// Manages the remote agent binary lifecycle.
pub struct AgentManager {
    session: Session,
}

impl AgentManager {
    pub fn new(session: Session) -> Self {
        Self { session }
    }

    /// Ensure the agent binary is up-to-date on the remote host, then spawn it.
    /// Returns a `ChannelStream` for reading the agent's stdout.
    ///
    /// If `local_agent_path` is provided, reads the binary from that file (development override).
    /// Otherwise, uses the embedded binary for the detected platform, falling back to
    /// `prebuilt-agents/` directory.
    pub async fn deploy_and_spawn(
        &self,
        local_agent_path: Option<&Path>,
    ) -> Result<ChannelStream<Msg>, SshError> {
        let platform = self.detect_platform().await?;
        let agent_bytes = self
            .resolve_agent_binary(&platform, local_agent_path)
            .await?;

        let local_hash = sha256_hex(&agent_bytes);

        let remote_dir = format!("{REMOTE_AGENT_DIR}/{}", platform.arch);
        let remote_path = format!("{remote_dir}/{REMOTE_AGENT_NAME}");

        let needs_upload = match self.remote_hash(&remote_path).await {
            Ok(remote_hash) => remote_hash != local_hash,
            Err(_) => true,
        };

        if needs_upload {
            self.upload(&agent_bytes, &remote_dir, &remote_path).await?;
        }

        // Kill any stale agent before spawning
        self.kill_stale_agent().await;

        self.spawn_agent(&remote_path).await
    }

    /// Resolve the agent binary bytes. Priority:
    /// 1. Explicit local path override (--agent-path)
    /// 2. Embedded binary from build.rs
    /// 3. Local prebuilt-agents/ directory
    async fn resolve_agent_binary(
        &self,
        platform: &Platform,
        local_override: Option<&Path>,
    ) -> Result<Cow<'static, [u8]>, SshError> {
        // 1. Explicit override
        if let Some(path) = local_override {
            let bytes = tokio::fs::read(path).await.map_err(|e| SshError::LocalIo {
                path: path.to_path_buf(),
                source: e,
            })?;
            return Ok(Cow::Owned(bytes));
        }

        // 2. Embedded binary
        if let Some(bytes) = crate::embedded::get_agent_binary(&platform.os, &platform.arch) {
            return Ok(Cow::Borrowed(bytes));
        }

        // 3. Fallback to prebuilt-agents/ directory next to the executable
        let exe = std::env::current_exe().unwrap_or_default();
        let exe_dir = exe.parent().unwrap_or_else(|| Path::new("."));
        let prebuilt_path = exe_dir
            .join("prebuilt-agents")
            .join(format!("{}-{}", platform.os, platform.arch))
            .join(REMOTE_AGENT_NAME);

        if prebuilt_path.exists() {
            let bytes = tokio::fs::read(&prebuilt_path)
                .await
                .map_err(|e| SshError::LocalIo {
                    path: prebuilt_path.clone(),
                    source: e,
                })?;
            return Ok(Cow::Owned(bytes));
        }

        Err(SshError::AgentDeploy(format!(
            "no agent binary available for {}-{} (no embedded binary, no prebuilt at {})",
            platform.os,
            platform.arch,
            prebuilt_path.display()
        )))
    }

    /// Detect the remote system OS and architecture via `uname -s` and `uname -m`.
    pub async fn detect_platform(&self) -> Result<Platform, SshError> {
        let output = self.session.exec("uname -sm").await?;

        let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();

        let mut parts = raw.split_whitespace();
        let raw_os = parts
            .next()
            .ok_or_else(|| SshError::AgentDeploy("could not detect remote OS".to_string()))?;
        let raw_arch = parts.next().ok_or_else(|| {
            SshError::AgentDeploy("could not detect remote architecture".to_string())
        })?;

        let os = normalize_os(raw_os);
        let arch = normalize_arch(raw_arch);

        Ok(Platform { os, arch })
    }

    /// Get SHA256 hash of the remote agent binary.
    async fn remote_hash(&self, remote_path: &str) -> Result<String, SshError> {
        // Try sha256sum first (Linux), fall back to openssl (macOS)
        let cmd = format!(
            "sha256sum '{remote_path}' 2>/dev/null || openssl dgst -sha256 '{remote_path}' 2>/dev/null"
        );
        let output = self.session.exec(&cmd).await?;

        if !output.success {
            return Err(SshError::AgentDeploy(format!(
                "remote agent not found at {remote_path}"
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        // sha256sum output: "hash  filename"
        // openssl output: "SHA256(filename)= hash"
        let hash = stdout
            .split_whitespace()
            .find(|s| s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit()))
            .ok_or_else(|| {
                SshError::AgentDeploy(format!("could not parse remote hash: {stdout}"))
            })?;

        Ok(hash.to_string())
    }

    /// Upload the agent binary to the remote host atomically.
    async fn upload(
        &self,
        bytes: &[u8],
        remote_dir: &str,
        remote_path: &str,
    ) -> Result<(), SshError> {
        // Ensure directory exists
        self.session
            .exec(&format!("mkdir -p '{remote_dir}'"))
            .await?;

        let tmp_path = format!("{remote_path}.tmp");

        // Upload via stdin pipe to temp file
        self.session
            .exec_with_stdin(&format!("cat > '{tmp_path}'"), bytes)
            .await?;

        // Atomic mv + chmod
        let output = self
            .session
            .exec(&format!(
                "mv '{tmp_path}' '{remote_path}' && chmod +x '{remote_path}'"
            ))
            .await?;

        if !output.success {
            return Err(SshError::AgentDeploy(format!(
                "failed to install agent: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }

        Ok(())
    }

    /// Kill any stale agent process from a previous session.
    async fn kill_stale_agent(&self) {
        // Read PID file
        let output = match self.session.exec(&format!("cat {REMOTE_PID_FILE}")).await {
            Ok(o) if o.success => o,
            _ => return,
        };

        let pid_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let pid: u32 = match pid_str.parse() {
            Ok(p) => p,
            Err(_) => return,
        };

        // Verify the PID is actually our agent (prevent killing unrelated process)
        let verify_cmd =
            format!("cat /proc/{pid}/comm 2>/dev/null || ps -p {pid} -o comm= 2>/dev/null");
        let output = match self.session.exec(&verify_cmd).await {
            Ok(o) => o,
            Err(_) => return,
        };

        let comm = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if comm == REMOTE_AGENT_NAME {
            let _ = self.session.exec(&format!("kill {pid_str}")).await;
        }
    }

    /// Spawn the remote agent as a persistent process.
    async fn spawn_agent(&self, remote_path: &str) -> Result<ChannelStream<Msg>, SshError> {
        self.session.exec_streaming(remote_path).await
    }

    /// Kill the remote agent gracefully (for shutdown).
    #[allow(dead_code)]
    pub async fn kill_remote_agent(&self) {
        self.kill_stale_agent().await;
    }
}

fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    hex_encode(&result)
}

fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// Normalize OS name from `uname -s` output.
fn normalize_os(raw: &str) -> String {
    match raw {
        "Linux" => "linux".to_string(),
        "Darwin" => "darwin".to_string(),
        other => other.to_lowercase(),
    }
}

/// Normalize architecture from `uname -m` output.
fn normalize_arch(raw: &str) -> String {
    match raw {
        "arm64" | "aarch64" => "aarch64".to_string(),
        "x86_64" | "amd64" => "x86_64".to_string(),
        other => other.to_lowercase(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_os_values() {
        assert_eq!(normalize_os("Linux"), "linux");
        assert_eq!(normalize_os("Darwin"), "darwin");
        assert_eq!(normalize_os("FreeBSD"), "freebsd");
    }

    #[test]
    fn normalize_arch_values() {
        assert_eq!(normalize_arch("x86_64"), "x86_64");
        assert_eq!(normalize_arch("amd64"), "x86_64");
        assert_eq!(normalize_arch("aarch64"), "aarch64");
        assert_eq!(normalize_arch("arm64"), "aarch64");
        assert_eq!(normalize_arch("armv7l"), "armv7l");
    }

    #[test]
    fn platform_target_triple() {
        let p = Platform {
            os: "linux".to_string(),
            arch: "x86_64".to_string(),
        };
        assert_eq!(p.target_triple(), "x86_64-unknown-linux-musl");

        let p = Platform {
            os: "darwin".to_string(),
            arch: "aarch64".to_string(),
        };
        assert_eq!(p.target_triple(), "aarch64-apple-darwin");

        let p = Platform {
            os: "linux".to_string(),
            arch: "aarch64".to_string(),
        };
        assert_eq!(p.target_triple(), "aarch64-unknown-linux-musl");
    }
}
