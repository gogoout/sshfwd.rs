mod app;
mod discovery;
pub mod embedded;
mod error;
mod event;
mod ssh;
mod ui;

use std::io;
use std::path::PathBuf;
use std::process;

use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::ExecutableCommand;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use app::{Message, Model};
use discovery::{DiscoveryEvent, DiscoveryStream};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: sshfwd <[user@]hostname> [--agent-path <path>]");
        process::exit(1);
    }

    let destination = args[1].clone();

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

    // Connect and start discovery before entering TUI
    eprintln!("Connecting to {destination}...");

    let session = match ssh::session::connect(&destination).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Connection failed: {e}");
            process::exit(1);
        }
    };

    // Leak session to get 'static lifetime. Safe because:
    // - process::exit(0) skips all destructors anyway (tui-architecture.md)
    // - Session's drop blocks on SSH master process, which we want to avoid
    let session: &'static openssh::Session = Box::leak(Box::new(session));

    eprintln!("Connected. Deploying agent...");

    let mut stream = match DiscoveryStream::start(session, agent_path.as_deref()).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Discovery failed: {e}");
            process::exit(1);
        }
    };

    // Install panic hook that restores terminal
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = terminal::disable_raw_mode();
        let _ = io::stdout().execute(LeaveAlternateScreen);
        original_hook(info);
    }));

    // Enter TUI mode
    terminal::enable_raw_mode().expect("failed to enable raw mode");
    io::stdout()
        .execute(EnterAlternateScreen)
        .expect("failed to enter alternate screen");

    let backend = CrosstermBackend::new(io::BufWriter::new(io::stdout()));
    let mut terminal = Terminal::new(backend).expect("failed to create terminal");

    let mut model = Model::new(destination);

    // Initial render
    terminal
        .draw(|frame| app::view(&model, frame))
        .expect("failed to draw");
    model.needs_render = false;

    // Keyboard channel — bounded(0) (rendezvous) so the keyboard thread
    // blocks on send() until the main loop is ready. No poll() needed;
    // bare read() avoids the use-dev-tty poll(ZERO) bug.
    let (kb_tx, kb_rx) = crossbeam_channel::bounded::<Message>(0);

    std::thread::spawn(move || loop {
        match crossterm::event::read() {
            Ok(evt) => {
                if let Some(msg) = event::crossterm_event_to_message(evt) {
                    if kb_tx.send(msg).is_err() {
                        break;
                    }
                }
            }
            Err(_) => {
                let _ = kb_tx.send(Message::Quit);
                break;
            }
        }
    });

    // Background channel — unbounded for infrequent discovery + tick events
    let (bg_tx, bg_rx) = crossbeam_channel::unbounded::<Message>();

    // Discovery task
    let disc_tx = bg_tx.clone();
    tokio::spawn(async move {
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
    });

    // Tick task
    let tick_tx = bg_tx.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            interval.tick().await;
            if tick_tx.send(Message::Tick).is_err() {
                break;
            }
        }
    });

    // Drop original sender so bg channel closes when all tasks finish
    drop(bg_tx);

    // Main loop: crossbeam::select! multiplexes keyboard + background channels
    while model.running {
        crossbeam_channel::select! {
            recv(kb_rx) -> msg => {
                match msg {
                    Ok(msg) => app::update(&mut model, msg),
                    Err(_) => break,
                }
            }
            recv(bg_rx) -> msg => {
                match msg {
                    Ok(msg) => app::update(&mut model, msg),
                    Err(_) => break,
                }
            }
        }

        if model.needs_render {
            terminal
                .draw(|frame| app::view(&model, frame))
                .expect("failed to draw");
            model.needs_render = false;
        }
    }

    // Restore terminal and exit immediately. Dropping crossterm's
    // EventStream blocks on a pending stdin read, and openssh's Session
    // destructor blocks on the SSH master process — so skip all
    // destructors via process::exit().
    terminal::disable_raw_mode().ok();
    io::stdout().execute(LeaveAlternateScreen).ok();
    process::exit(0);
}
