use std::env;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;

use elesdee_core::link::UiPackageState;
use elesdee_core::{AppState, LinkService, diagnostics, router, start_usage_poller};
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

const DEFAULT_PORT: u16 = 4319;

#[tokio::main]
async fn main() -> ExitCode {
    let port = match port_from_environment() {
        Ok(port) => port,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::FAILURE;
        }
    };

    init_tracing();
    let state = AppState::new_with_port(port);
    let ui = UiPackageState::from_directory(phone_ui_directory().as_deref());
    if let Some(error) = ui.error() {
        tracing::warn!(%error, "the phone UI package is not served");
    }
    let link = match LinkService::start(state.clone(), ui).await {
        Ok(link) => Some(link),
        Err(error) => {
            tracing::error!(%error, "the phone link is unavailable; pairing returns 503");
            None
        }
    };
    start_usage_poller(state.clone());
    let bootstrap = state.bootstrap();
    let app = router(state.clone());
    let requested = SocketAddr::from(([127, 0, 0, 1], port));

    let listener = match TcpListener::bind(requested).await {
        Ok(listener) => listener,
        Err(error) => {
            tracing::error!(%requested, %error, "could not bind the core service");
            return ExitCode::FAILURE;
        }
    };
    let address = listener.local_addr().unwrap_or(requested);

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        %address,
        projects = bootstrap.projects.len(),
        providers = bootstrap.providers.len(),
        configs = bootstrap.configs.len(),
        sessions = bootstrap.sessions.len(),
        "core service started"
    );

    tokio::select! {
        result = axum::serve(listener, app) => {
            if let Err(error) = result {
                tracing::error!(%error, "core service stopped unexpectedly");
                if !state.shutdown_all() {
                    tracing::warn!("session shutdown timed out");
                }
                state.stop_all_browsers().await;
                return ExitCode::FAILURE;
            }
        }
        _ = shutdown_signal() => {
            tracing::info!("shutdown signal received");
        }
    }

    if let Some(link) = link {
        link.shutdown().await;
    }
    state.stop_all_browsers().await;
    if !state.shutdown_all() {
        tracing::warn!("session shutdown timed out");
    }
    tracing::info!("core service stopped");
    ExitCode::SUCCESS
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let stderr_layer = tracing_subscriber::fmt::layer()
        .json()
        .with_ansi(false)
        .with_writer(std::io::stderr);
    match diagnostics::writer_from_environment() {
        Some(writer) => {
            let file_layer = tracing_subscriber::fmt::layer()
                .json()
                .with_ansi(false)
                .with_writer(writer);
            tracing_subscriber::registry()
                .with(filter)
                .with(stderr_layer)
                .with(file_layer)
                .init();
        }
        None => {
            tracing_subscriber::registry()
                .with(filter)
                .with(stderr_layer)
                .init();
        }
    }
}

fn port_from_environment() -> Result<u16, String> {
    let mut arguments = env::args().skip(1);
    let mut command_line = None;

    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--port" => {
                let value = arguments
                    .next()
                    .ok_or_else(|| "--port needs a value.".to_owned())?;
                command_line = Some(parse_port(&value)?);
            }
            "--phone-ui" => {
                arguments
                    .next()
                    .ok_or_else(|| "--phone-ui needs a folder.".to_owned())?;
            }
            "--help" | "-h" => {
                println!("Usage: elesdee-core [--port PORT] [--phone-ui DIR]");
                println!("The service binds 127.0.0.1 only.");
                std::process::exit(0);
            }
            other => {
                if let Some(value) = other.strip_prefix("--port=") {
                    command_line = Some(parse_port(value)?);
                } else if other.starts_with("--phone-ui=") {
                } else {
                    return Err(format!("Unknown argument: {other}"));
                }
            }
        }
    }

    if let Some(port) = command_line {
        return Ok(port);
    }

    match env::var("ELESDEE_PORT") {
        Ok(value) if !value.trim().is_empty() => parse_port(value.trim()),
        _ => Ok(DEFAULT_PORT),
    }
}

/// Returns the phone UI package folder.
///
/// The desktop shell passes it on the command line. Development falls back to
/// the environment and then to the build output.
fn phone_ui_directory() -> Option<PathBuf> {
    let mut arguments = env::args().skip(1);
    while let Some(argument) = arguments.next() {
        if argument == "--phone-ui" {
            return arguments.next().map(PathBuf::from);
        }
        if let Some(value) = argument.strip_prefix("--phone-ui=") {
            return Some(PathBuf::from(value));
        }
    }
    if let Some(value) = env::var_os("ELESDEE_PHONE_UI").filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(value));
    }
    let fallback = PathBuf::from("app/dist-phone-package");
    fallback.is_dir().then_some(fallback)
}

fn parse_port(value: &str) -> Result<u16, String> {
    let port = value
        .parse::<u16>()
        .map_err(|_| format!("Invalid port: {value}"))?;
    if port == 0 {
        return Err("Port must be between 1 and 65535.".to_owned());
    }
    Ok(port)
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        match signal(SignalKind::terminate()) {
            Ok(mut terminate) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = terminate.recv() => {}
                }
            }
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }

    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
