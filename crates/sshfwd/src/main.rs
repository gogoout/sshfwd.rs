mod app;
mod discovery;
pub mod embedded;
mod error;
mod event;
mod forward;
mod notify;
mod ssh;
mod ui;

use std::io;
use std::path::PathBuf;
use std::process;

use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::ExecutableCommand;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use app::{Message, Model};
use discovery::{DiscoveryEvent, DiscoveryStream};
use forward::persistence;
use forward::{ForwardEntry, ForwardManager, ForwardStatus};

fn main() {
    // Single-threaded runtime: no worker pool, no work-stealing overhead.
    // Moves to a dedicated OS thread for discovery I/O after setup.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to create tokio runtime");

    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: sshfwd <[user@]hostname> [--agent-path <path>] [--no-notify]");
        process::exit(1);
    }

    let destination = args[1].clone();

    let agent_path = args
        .iter()
        .position(|a| a == "--agent-path")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from);

    let no_notify = args.iter().any(|a| a == "--no-notify");

    if let Some(ref path) = agent_path {
        if !path.exists() {
            eprintln!(
                "Agent binary not found at: {}\nBuild it with: cargo build -p sshfwd-agent",
                path.display()
            );
            process::exit(1);
        }
    }

    // Connect and start discovery before entering TUI
    eprintln!("Connecting to {destination}...");

    let (mut stream, session) = runtime.block_on(async {
        let session = match ssh::session::Session::connect(&destination).await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Connection failed: {e}");
                process::exit(1);
            }
        };

        eprintln!("Connected. Deploying agent...");

        // Clone session before discovery consumes it
        let session_for_fwd = session.clone();

        let stream = match DiscoveryStream::start(session, agent_path.as_deref()).await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Discovery failed: {e}");
                process::exit(1);
            }
        };

        (stream, session_for_fwd)
    });

    // Install panic hook that restores terminal
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = io::stdout().execute(DisableMouseCapture);
        let _ = terminal::disable_raw_mode();
        let _ = io::stdout().execute(LeaveAlternateScreen);
        original_hook(info);
    }));

    // Enter TUI mode
    terminal::enable_raw_mode().expect("failed to enable raw mode");
    io::stdout()
        .execute(EnterAlternateScreen)
        .expect("failed to enter alternate screen");
    io::stdout()
        .execute(EnableMouseCapture)
        .expect("failed to enable mouse capture");

    let backend = CrosstermBackend::new(io::BufWriter::new(io::stdout()));
    let mut terminal = Terminal::new(backend).expect("failed to create terminal");

    let mut model = Model::new(destination.clone());
    model.notifications_enabled = !no_notify;

    // Load persisted forwards (all start as Paused — first scan triggers activation)
    let persisted = persistence::load_forwards(&destination);
    for pf in persisted {
        model.forwards.insert(
            pf.remote_port,
            ForwardEntry {
                local_port: pf.local_port,
                status: ForwardStatus::Paused,
                active_connections: 0,
            },
        );
    }

    // Initial render
    terminal
        .draw(|frame| app::view(&mut model, frame))
        .expect("failed to draw");
    model.needs_render = false;

    // Keyboard channel — bounded(0) (rendezvous) so the keyboard thread
    // blocks on send() until the main loop is ready. No poll() needed;
    // bare read() avoids the use-dev-tty poll(ZERO) bug.
    let (kb_tx, kb_rx) = crossbeam_channel::bounded::<Message>(0);

    std::thread::spawn(move || {
        while let Ok(evt) = crossterm::event::read() {
            if let Some(msg) = event::crossterm_event_to_message(evt) {
                if kb_tx.send(msg).is_err() {
                    break;
                }
            }
        }
    });

    // Background channel — unbounded for infrequent discovery + tick + forward events
    let (bg_tx, bg_rx) = crossbeam_channel::unbounded::<Message>();

    // Forward command channel (sync → async)
    let (fwd_cmd_tx, fwd_cmd_rx) = tokio::sync::mpsc::unbounded_channel();

    // Discovery + ForwardManager — share a single-threaded tokio runtime on one OS thread
    let disc_tx = bg_tx.clone();
    let fwd_event_tx = bg_tx.clone();
    std::thread::spawn(move || {
        runtime.block_on(async move {
            // Spawn ForwardManager as a tokio task on this runtime
            let fwd_manager = ForwardManager::new(session, fwd_cmd_rx, fwd_event_tx);
            let fwd_handle = tokio::spawn(fwd_manager.run());

            // Run discovery loop
            loop {
                match stream.next_event().await {
                    Some(DiscoveryEvent::Scan(scan)) => {
                        if disc_tx.send(Message::ScanReceived(scan)).is_err() {
                            break;
                        }
                    }
                    Some(DiscoveryEvent::Warning(msg)) => {
                        if disc_tx.send(Message::DiscoveryWarning(msg)).is_err() {
                            break;
                        }
                    }
                    Some(DiscoveryEvent::Error(e)) => {
                        let _ = disc_tx.send(Message::DiscoveryError(e));
                        break;
                    }
                    None => {
                        let _ = disc_tx.send(Message::StreamEnded);
                        break;
                    }
                }
            }

            // Keep runtime alive for ForwardManager after discovery ends
            let _ = fwd_handle.await;
        });
    });

    // Tick thread — plain OS thread, no async needed
    let tick_tx = bg_tx.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
        if tick_tx.send(Message::Tick).is_err() {
            break;
        }
    });

    // Drop original sender so bg channel closes when all tasks finish
    drop(bg_tx);

    // Main loop on the main OS thread — completely independent of tokio.
    // crossbeam::select! multiplexes keyboard + background channels.
    while model.running {
        crossbeam_channel::select! {
            recv(kb_rx) -> msg => {
                match msg {
                    Ok(msg) => {
                        let cmds = app::update(&mut model, msg);
                        for cmd in cmds {
                            let _ = fwd_cmd_tx.send(cmd);
                        }
                    }
                    Err(_) => break,
                }
            }
            recv(bg_rx) -> msg => {
                match msg {
                    Ok(msg) => {
                        let cmds = app::update(&mut model, msg);
                        for cmd in cmds {
                            let _ = fwd_cmd_tx.send(cmd);
                        }
                    }
                    Err(_) => break,
                }
            }
        }

        if model.needs_render {
            terminal
                .draw(|frame| app::view(&mut model, frame))
                .expect("failed to draw");
            model.needs_render = false;
        }
    }

    // Restore terminal and exit immediately. Dropping crossterm's
    // read() thread has no clean cancellation — so skip all
    // destructors via process::exit().
    io::stdout().execute(DisableMouseCapture).ok();
    terminal::disable_raw_mode().ok();
    io::stdout().execute(LeaveAlternateScreen).ok();
    process::exit(0);
}
