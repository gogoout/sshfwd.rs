use serde::{Deserialize, Serialize};

/// A listening port discovered on the remote host.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ListeningPort {
    pub protocol: Protocol,
    pub local_addr: String,
    pub port: u16,
    pub process: Option<ProcessInfo>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Tcp,
    Tcp6,
}

/// Information about the process owning a listening socket.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub cmdline: String,
    pub uid: u32,
}

/// A single scan snapshot from the agent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScanResult {
    pub agent_version: String,
    pub hostname: String,
    pub username: String,
    pub is_root: bool,
    pub ports: Vec<ListeningPort>,
    pub warnings: Vec<String>,
    pub scan_index: u64,
}

/// Top-level response envelope from the agent (one per JSON line).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum AgentResponse {
    Ok(ScanResult),
    Error(AgentError),
}

/// An error reported by the agent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentError {
    pub kind: AgentErrorKind,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentErrorKind {
    ScanFailed,
    PermissionDenied,
    Unsupported,
}

impl std::fmt::Display for AgentErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ScanFailed => write!(f, "scan_failed"),
            Self::PermissionDenied => write!(f, "permission_denied"),
            Self::Unsupported => write!(f, "unsupported"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_scan_result() -> ScanResult {
        ScanResult {
            agent_version: "0.1.0".to_string(),
            hostname: "server1".to_string(),
            username: "deploy".to_string(),
            is_root: false,
            ports: vec![
                ListeningPort {
                    protocol: Protocol::Tcp,
                    local_addr: "127.0.0.1".to_string(),
                    port: 5432,
                    process: Some(ProcessInfo {
                        pid: 1234,
                        name: "postgres".to_string(),
                        cmdline: "/usr/lib/postgresql/15/bin/postgres".to_string(),
                        uid: 108,
                    }),
                },
                ListeningPort {
                    protocol: Protocol::Tcp6,
                    local_addr: "::".to_string(),
                    port: 8080,
                    process: None,
                },
            ],
            warnings: vec!["permission denied reading /proc/999/fd".to_string()],
            scan_index: 42,
        }
    }

    #[test]
    fn scan_result_round_trip() {
        let result = sample_scan_result();
        let json = serde_json::to_string(&result).unwrap();
        let deserialized: ScanResult = serde_json::from_str(&json).unwrap();
        assert_eq!(result, deserialized);
    }

    #[test]
    fn agent_response_ok_round_trip() {
        let response = AgentResponse::Ok(sample_scan_result());
        let json = serde_json::to_string(&response).unwrap();
        let deserialized: AgentResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(response, deserialized);
    }

    #[test]
    fn agent_response_error_round_trip() {
        let response = AgentResponse::Error(AgentError {
            kind: AgentErrorKind::ScanFailed,
            message: "failed to read /proc/net/tcp".to_string(),
        });
        let json = serde_json::to_string(&response).unwrap();
        let deserialized: AgentResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(response, deserialized);
    }

    #[test]
    fn agent_response_ok_json_structure() {
        let response = AgentResponse::Ok(ScanResult {
            agent_version: "0.1.0".to_string(),
            hostname: "h".to_string(),
            username: "u".to_string(),
            is_root: true,
            ports: vec![],
            warnings: vec![],
            scan_index: 0,
        });
        let json = serde_json::to_string(&response).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["status"], "ok");
        assert_eq!(value["agent_version"], "0.1.0");
    }

    #[test]
    fn agent_response_error_json_structure() {
        let response = AgentResponse::Error(AgentError {
            kind: AgentErrorKind::PermissionDenied,
            message: "access denied".to_string(),
        });
        let json = serde_json::to_string(&response).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["status"], "error");
        assert_eq!(value["kind"], "permission_denied");
    }

    #[test]
    fn protocol_serialization() {
        assert_eq!(serde_json::to_string(&Protocol::Tcp).unwrap(), "\"tcp\"");
        assert_eq!(serde_json::to_string(&Protocol::Tcp6).unwrap(), "\"tcp6\"");
    }

    #[test]
    fn listening_port_without_process() {
        let port = ListeningPort {
            protocol: Protocol::Tcp,
            local_addr: "0.0.0.0".to_string(),
            port: 80,
            process: None,
        };
        let json = serde_json::to_string(&port).unwrap();
        let deserialized: ListeningPort = serde_json::from_str(&json).unwrap();
        assert_eq!(port, deserialized);
        assert!(json.contains("\"process\":null"));
    }
}
