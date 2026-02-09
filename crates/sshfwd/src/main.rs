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
use std::time::Duration;

use crossterm::event::EventStream;
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::ExecutableCommand;
use futures::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use app::{Message, Model};
use discovery::{DiscoveryEvent, DiscoveryStream};

const TICK_RATE: Duration = Duration::from_millis(16);

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

    eprintln!("Connected. Deploying agent...");

    let mut stream = match DiscoveryStream::start(&session, agent_path.as_deref()).await {
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

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend).expect("failed to create terminal");

    let mut model = Model::new(destination);
    let mut crossterm_reader = EventStream::new();
    let mut tick_interval = tokio::time::interval(TICK_RATE);

    // Initial render
    terminal
        .draw(|frame| app::view(&model, frame))
        .expect("failed to draw");
    model.needs_render = false;

    // Main event loop
    while model.running {
        let msg = tokio::select! {
            _ = tick_interval.tick() => {
                Some(Message::Tick)
            }
            event = crossterm_reader.next() => {
                match event {
                    Some(Ok(evt)) => event::crossterm_event_to_message(evt),
                    Some(Err(_)) => Some(Message::Quit),
                    None => Some(Message::Quit),
                }
            }
            event = stream.next_event() => {
                match event {
                    Some(DiscoveryEvent::Scan(scan)) => Some(Message::ScanReceived(scan)),
                    Some(DiscoveryEvent::Warning(msg)) => Some(Message::DiscoveryWarning(msg)),
                    Some(DiscoveryEvent::Error(e)) => Some(Message::DiscoveryError(e)),
                    None => Some(Message::StreamEnded),
                }
            }
        };

        if let Some(msg) = msg {
            app::update(&mut model, msg);
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
