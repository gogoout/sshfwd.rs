mod scanner;

use std::io::{self, Write};
use std::thread;
use std::time::Duration;

use sshfwd_common::types::{AgentError, AgentErrorKind, AgentResponse};

const SCAN_INTERVAL: Duration = Duration::from_secs(2);

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--version") {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return;
    }

    let once = args.iter().any(|a| a == "--once");

    write_pid_file();

    let mut scanner = scanner::create_scanner();
    let stdout = io::stdout();

    loop {
        let response = match scanner.scan() {
            Ok(result) => AgentResponse::Ok(result),
            Err(e) => AgentResponse::Error(e),
        };

        let line = match serde_json::to_string(&response) {
            Ok(json) => json,
            Err(e) => {
                let err_response = AgentResponse::Error(AgentError {
                    kind: AgentErrorKind::ScanFailed,
                    message: format!("failed to serialize scan result: {e}"),
                });
                serde_json::to_string(&err_response).unwrap()
            }
        };

        // Exit on broken pipe (SSH disconnect)
        let mut handle = stdout.lock();
        if writeln!(handle, "{line}").is_err() {
            break;
        }
        if handle.flush().is_err() {
            break;
        }

        if once {
            break;
        }

        thread::sleep(SCAN_INTERVAL);
    }
}

fn write_pid_file() {
    let dir = match home_dir() {
        Some(h) => h.join(".sshfwd"),
        None => return,
    };
    let _ = std::fs::create_dir_all(&dir);
    let pid_file = dir.join("agent.pid");
    let _ = std::fs::write(pid_file, std::process::id().to_string());
}

fn home_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(std::path::PathBuf::from)
}
