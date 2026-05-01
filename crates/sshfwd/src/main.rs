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
use forward::{ForwardEntry, ForwardKey, ForwardManager, ForwardStatus};

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

    // Channel for incoming reverse-forwarded connections from the SSH server.
    // Created before Session::connect so ClientHandler can deliver incoming channels.
    let (forwarded_tx, forwarded_rx) =
        tokio::sync::mpsc::unbounded_channel::<crate::ssh::session::IncomingForward>();

    // Connect and start discovery before entering TUI
    eprintln!("Connecting to {destination}...");

    let (initial_stream, session) = runtime.block_on(async {
        let session = match ssh::session::Session::connect(&destination, Some(forwarded_tx)).await {
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
        let key = ForwardKey {
            kind: pf.kind,
            remote_port: pf.remote_port,
        };
        model.forwards.insert(
            key,
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

    // Forward command channel (sync → async).
    // The receiver is owned by the sidecar; it is reused across reconnect cycles
    // so that the model's command stream is never interrupted.
    let (fwd_cmd_tx, fwd_cmd_rx) = tokio::sync::mpsc::unbounded_channel();

    // Discovery + ForwardManager sidecar with transparent reconnect.
    let disc_tx = bg_tx.clone();
    let fwd_event_tx = bg_tx.clone();
    std::thread::spawn(move || {
        runtime.block_on(run_sidecar(
            initial_stream,
            session,
            forwarded_rx,
            fwd_cmd_rx,
            disc_tx,
            fwd_event_tx,
            destination,
            agent_path,
        ));
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

/// If no discovery event arrives within this window, the session is treated as dead.
/// The agent scans every ~2 s; 12 s gives 6× headroom before forcing a reconnect.
const DISCOVERY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(12);

/// Run one session cycle: drive discovery and ForwardManager concurrently.
///
/// Returns when the discovery stream ends or times out.  The caller is responsible for
/// reconnecting and calling this again with a fresh session / stream.
/// `forwarded_rx` is consumed so callers can provide a fresh one on reconnect.
async fn run_session_cycle(
    mut stream: DiscoveryStream,
    session: ssh::session::Session,
    mut forwarded_rx: tokio::sync::mpsc::UnboundedReceiver<crate::ssh::session::IncomingForward>,
    fwd_cmd_rx: &mut tokio::sync::mpsc::UnboundedReceiver<forward::ForwardCommand>,
    disc_tx: crossbeam_channel::Sender<Message>,
    fwd_event_tx: crossbeam_channel::Sender<Message>,
) {
    // Spawn local port scanner (aborted when this cycle ends).
    let local_scan = discovery::local::spawn_local_scan(disc_tx.clone());

    let manager = ForwardManager::new(session, fwd_event_tx);
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    let manager_fut = manager.run(fwd_cmd_rx, &mut forwarded_rx, shutdown_rx);
    tokio::pin!(manager_fut);

    loop {
        tokio::select! {
            _ = &mut manager_fut => {
                // Manager exited (cmd_tx closed or shutdown signal already fired).
                break;
            }
            result = tokio::time::timeout(DISCOVERY_TIMEOUT, stream.next_event()) => {
                match result {
                    Ok(Some(DiscoveryEvent::Scan(scan))) => {
                        disc_tx.send(Message::ScanReceived(scan)).ok();
                    }
                    Ok(Some(DiscoveryEvent::Warning(w))) => {
                        disc_tx.send(Message::DiscoveryWarning(w)).ok();
                    }
                    Ok(Some(DiscoveryEvent::Error(e))) => {
                        disc_tx.send(Message::DiscoveryError(e)).ok();
                        let _ = shutdown_tx.send(());
                        (&mut manager_fut).await;
                        break;
                    }
                    Err(_) => {
                        // No event within DISCOVERY_TIMEOUT — treat as dead session.
                        let _ = shutdown_tx.send(());
                        (&mut manager_fut).await;
                        break;
                    }
                    Ok(None) => unreachable!("next_event always returns Some"),
                }
            }
        }
    }

    local_scan.abort();
}

/// Reconnect with exponential backoff until a new SSH session is established.
/// Returns the new session together with its paired forwarded-channel receiver.
/// The first attempt is immediate; sleep only occurs after a failed attempt.
async fn reconnect_with_backoff(
    destination: &str,
    disc_tx: &crossbeam_channel::Sender<Message>,
    backoff: &mut std::time::Duration,
) -> (
    ssh::session::Session,
    tokio::sync::mpsc::UnboundedReceiver<crate::ssh::session::IncomingForward>,
) {
    loop {
        disc_tx.send(Message::Reconnecting).ok();

        let (ftx, frx) =
            tokio::sync::mpsc::unbounded_channel::<crate::ssh::session::IncomingForward>();
        match ssh::session::Session::connect(destination, Some(ftx)).await {
            Ok(new_session) => {
                *backoff = std::time::Duration::from_secs(1);
                return (new_session, frx);
            }
            Err(_) => {
                tokio::time::sleep(*backoff).await;
                *backoff = (*backoff * 2).min(std::time::Duration::from_secs(30));
            }
        }
    }
}

/// Top-level sidecar: outer reconnect loop wrapping session cycles.
#[allow(clippy::too_many_arguments)]
async fn run_sidecar(
    initial_stream: DiscoveryStream,
    initial_session: ssh::session::Session,
    initial_forwarded_rx: tokio::sync::mpsc::UnboundedReceiver<
        crate::ssh::session::IncomingForward,
    >,
    mut fwd_cmd_rx: tokio::sync::mpsc::UnboundedReceiver<forward::ForwardCommand>,
    disc_tx: crossbeam_channel::Sender<Message>,
    fwd_event_tx: crossbeam_channel::Sender<Message>,
    destination: String,
    agent_path: Option<PathBuf>,
) {
    let mut session = initial_session;
    let mut stream = initial_stream;
    let mut forwarded_rx = initial_forwarded_rx;
    let mut backoff = std::time::Duration::from_secs(1);

    loop {
        // Notify model of fresh session — triggers Reverse forward reactivation
        // (also fires on initial startup so persisted Reverse forwards activate).
        disc_tx.send(Message::Reconnected).ok();

        // Run one session cycle (blocks until stream ends).
        run_session_cycle(
            stream,
            session.clone(),
            forwarded_rx,
            &mut fwd_cmd_rx,
            disc_tx.clone(),
            fwd_event_tx.clone(),
        )
        .await;

        // Session ended — notify model.
        disc_tx.send(Message::ConnectionLost).ok();

        // Reconnect with backoff; first attempt is immediate.
        (session, forwarded_rx) =
            reconnect_with_backoff(&destination, &disc_tx, &mut backoff).await;

        // Deploy agent on the new session.
        stream = loop {
            match DiscoveryStream::start(session.clone(), agent_path.as_deref()).await {
                Ok(s) => break s,
                Err(_) => {
                    // Agent deploy failed — treat as another connection loss.
                    disc_tx.send(Message::ConnectionLost).ok();
                    (session, forwarded_rx) =
                        reconnect_with_backoff(&destination, &disc_tx, &mut backoff).await;
                }
            }
        };
        // Reconnected is sent at the top of the loop, before run_session_cycle.
    }
}
