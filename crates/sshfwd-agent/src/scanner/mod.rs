use sshfwd_common::types::{AgentError, ScanResult};

// Pure parsing logic — always compiled for testing on any platform
pub mod proc_net_tcp;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "macos")]
pub mod macos;

pub trait Scanner {
    fn scan(&mut self) -> Result<ScanResult, AgentError>;
}

/// Create the platform-appropriate scanner.
pub fn create_scanner() -> Box<dyn Scanner> {
    #[cfg(target_os = "linux")]
    {
        Box::new(linux::LinuxScanner::new())
    }
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::MacosScanner::new())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        compile_error!("unsupported target OS for sshfwd-agent");
    }
}
