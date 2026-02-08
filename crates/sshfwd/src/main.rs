mod discovery;
pub mod embedded;
mod error;
mod ssh;

use std::path::PathBuf;
use std::process;

use discovery::{DiscoveryEvent, DiscoveryStream};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: sshfwd <[user@]hostname> [--agent-path <path>]");
        process::exit(1);
    }

    let destination = &args[1];

    // Optional: override agent binary path for development
    let agent_path = args
        .iter()
        .position(|a| a == "--agent-path")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from);

    if let Some(ref path) = agent_path {
        if !path.exists() {
            eprintln!(
                "Agent binary not found at: {}\nBuild it with: cargo build -p sshfwd-agent",
                path.display()
            );
            process::exit(1);
        }
    }

    eprintln!("Connecting to {destination}...");

    let session = match ssh::session::connect(destination).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Connection failed: {e}");
            process::exit(1);
        }
    };

    eprintln!("Connected. Deploying agent...");

    let mut stream = match DiscoveryStream::start(&session, agent_path.as_deref()).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Discovery failed: {e}");
            process::exit(1);
        }
    };

    eprintln!("Agent running. Streaming port snapshots (Ctrl+C to stop)...\n");

    loop {
        tokio::select! {
            event = stream.next_event() => {
                match event {
                    Some(DiscoveryEvent::Scan(scan)) => {
                        println!(
                            "[scan #{}] {}@{} | {} ports | {} warnings",
                            scan.scan_index,
                            scan.username,
                            scan.hostname,
                            scan.ports.len(),
                            scan.warnings.len()
                        );
                        for port in &scan.ports {
                            let proc_info = port.process.as_ref().map_or(
                                "unknown".to_string(),
                                |p| format!("{} (pid {})", p.name, p.pid),
                            );
                            println!(
                                "  {:>5}/{:<4} {} -> {}",
                                port.port,
                                format!("{:?}", port.protocol).to_lowercase(),
                                port.local_addr,
                                proc_info
                            );
                        }
                        for warning in &scan.warnings {
                            eprintln!("  warning: {warning}");
                        }
                        println!();
                    }
                    Some(DiscoveryEvent::Warning(msg)) => {
                        eprintln!("warning: {msg}");
                    }
                    Some(DiscoveryEvent::Error(e)) => {
                        eprintln!("error: {e}");
                        break;
                    }
                    None => break,
                }
            }
            _ = tokio::signal::ctrl_c() => {
                eprintln!("\nShutting down...");
                break;
            }
        }
    }

    // Kill remote agent on exit
    let manager = ssh::agent::AgentManager::new(&session);
    manager.kill_remote_agent().await;

    session.close().await.ok();
    eprintln!("Done.");
}
