use std::path::{Path, PathBuf};

use ssh2_config::{ParseRule, SshConfig};

pub struct ResolvedConfig {
    pub hostname: Option<String>,
    pub port: Option<u16>,
    pub user: Option<String>,
    pub proxy_jump: Option<String>,
    pub identity_files: Vec<PathBuf>,
}

/// Resolve SSH config for a host by parsing `~/.ssh/config`.
///
/// Returns default (empty) config if the file is missing or unparseable.
pub fn resolve_host_config(host: &str) -> ResolvedConfig {
    let cfg = SshConfig::parse_default_file(
        ParseRule::ALLOW_UNKNOWN_FIELDS | ParseRule::ALLOW_UNSUPPORTED_FIELDS,
    )
    .ok();

    let params = cfg.as_ref().map(|c| c.query(host));

    match params {
        Some(params) => {
            let identity_files = params
                .identity_file
                .as_ref()
                .map(|files| files.iter().map(|p| expand_tilde(p)).collect())
                .unwrap_or_default();

            let proxy_jump = params
                .proxy_jump
                .as_ref()
                .and_then(|jumps| jumps.first().map(|j| j.to_string()));

            ResolvedConfig {
                hostname: params.host_name.clone(),
                port: params.port,
                user: params.user.clone(),
                proxy_jump,
                identity_files,
            }
        }
        None => ResolvedConfig {
            hostname: None,
            port: None,
            user: None,
            proxy_jump: None,
            identity_files: Vec::new(),
        },
    }
}

/// Parse `user@host` into `(Option<user>, host)`.
pub fn parse_destination(destination: &str) -> (Option<String>, String) {
    if let Some((user, host)) = destination.split_once('@') {
        (Some(user.to_string()), host.to_string())
    } else {
        (None, destination.to_string())
    }
}

/// Expand leading `~` to `$HOME` in a path.
fn expand_tilde(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(format!("{home}/{rest}"));
        }
    }
    path.to_path_buf()
}
