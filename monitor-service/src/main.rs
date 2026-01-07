mod process_win;

use axum::{
    extract::Json,
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use clap::{Parser, Subcommand};
use monitor_lib::{display, MonitorInfo, TopologyMode};
use rust_embed::RustEmbed;
use std::net::SocketAddr;
use tower_http::trace::TraceLayer;

#[derive(Parser, Clone)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Run as a Windows Service
    #[arg(long)]
    service: bool,

    /// IP address and port to listen on (e.g. 0.0.0.0:3000)
    #[arg(long, default_value = "0.0.0.0:3000")]
    addr: String,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Clone)]
enum Commands {
    /// Run as Agent (Display operations)
    Agent {
        #[command(subcommand)]
        command: AgentCommands,
    },
}

#[derive(Subcommand, Clone)]
enum AgentCommands {
    /// List all monitors
    List,
    /// Set topology mode
    Set {
        /// Mode: internal, external, extend, clone, or single:N
        mode: String,
    },
}

#[derive(RustEmbed)]
#[folder = "assets/"]
struct Assets;

#[cfg(windows)]
use windows_service::{
    define_windows_service,
    service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
        ServiceType,
    },
    service_control_handler::{self, ServiceControlHandlerResult},
    service_dispatcher,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    
    // Dispatch Command
    if let Some(Commands::Agent { command }) = cli.command {
        return run_agent(command).await;
    }

    if cli.service {
        #[cfg(windows)]
        run_service()?;
        #[cfg(not(windows))]
        println!("Service mode not supported on non-Windows OS");
    } else {
        // Run in console mode for testing
        run_server(None, cli.addr.parse()?).await?;
    }
    Ok(())
}

async fn run_agent(cmd: AgentCommands) -> anyhow::Result<()> {
    match cmd {
        AgentCommands::List => {
            let monitors = display::list_monitors().map_err(|e| anyhow::anyhow!(e))?;
            println!("{}", serde_json::to_string_pretty(&monitors)?);
        }
        AgentCommands::Set { mode } => {
            let topology_mode = if mode.starts_with("single:") {
                let id_str = mode.trim_start_matches("single:");
                // Don't parse as u32 anymore, pass the raw string "low:high:id"
                TopologyMode::Single(id_str.to_string())
            } else {
                match mode.to_lowercase().as_str() {
                    "internal" => TopologyMode::Internal,
                    "external" => TopologyMode::External,
                    "extend" => TopologyMode::Extend,
                    "clone" => TopologyMode::Clone,
                    _ => return Err(anyhow::anyhow!("Invalid mode")),
                }
            };

            display::set_topology(topology_mode).map_err(|e| anyhow::anyhow!(e))?;
            println!("Success");
        }
    }
    Ok(())
}

#[cfg(windows)]
define_windows_service!(ffi_service_main, my_service_main);

#[cfg(windows)]
fn run_service() -> windows_service::Result<()> {
    service_dispatcher::start("MonitorService", ffi_service_main)
}

#[cfg(windows)]
fn my_service_main(_arguments: Vec<std::ffi::OsString>) {
    if let Err(_e) = run_service_impl() {
        // Log error
    }
}

#[cfg(windows)]
fn run_service_impl() -> anyhow::Result<()> {
    // Parse arguments again because we can't easily pass them from main()
    // This allows configuring the service via ImagePath arguments
    // e.g. sc config MonitorService binPath= "C:\path\to.exe --service --addr 1.2.3.4:80"
    let cli = Cli::parse();

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let mut shutdown_tx = Some(shutdown_tx);

    let event_handler = move |control_event| -> ServiceControlHandlerResult {
        match control_event {
            ServiceControl::Stop => {
                if let Some(tx) = shutdown_tx.take() {
                    let _ = tx.send(());
                }
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    };

    let status_handle = service_control_handler::register("MonitorService", event_handler)?;

    status_handle.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Running,
        controls_accepted: ServiceControlAccept::STOP,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: std::time::Duration::default(),
        process_id: None,
    })?;

    // Spawn the server in a separate runtime/task
    let rt = tokio::runtime::Runtime::new()?;
    let addr = cli.addr.parse().unwrap_or_else(|_| "0.0.0.0:3000".parse().unwrap());
    
    rt.block_on(async {
        if let Err(e) = run_server(Some(shutdown_rx), addr).await {
            eprintln!("Server error: {}", e);
        }
    });

    status_handle.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Stopped,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: std::time::Duration::default(),
        process_id: None,
    })?;

    Ok(())
}

async fn run_server(shutdown_rx: Option<tokio::sync::oneshot::Receiver<()>>, addr: SocketAddr) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/api/monitors", get(list_monitors))
        .route("/api/topology", post(set_topology_handler))
        .fallback(serve_assets)
        .layer(TraceLayer::new_for_http());

    println!("Listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    
    if let Some(rx) = shutdown_rx {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                rx.await.ok();
            })
            .await?;
    } else {
        // Run without graceful shutdown handler (e.g. console mode, Ctrl+C handled by OS)
        axum::serve(listener, app).await?;
    }
    
    Ok(())
}

async fn list_monitors() -> Json<Vec<MonitorInfo>> {
    // Attempt to run via Agent in Active Session first
    // This is required when running as a Service (Session 0)
    if let Ok(json_str) = process_win::run_agent_in_active_session(&["list"]) {
        if let Ok(monitors) = serde_json::from_str::<Vec<MonitorInfo>>(&json_str) {
             return Json(monitors);
        }
    }
    
    // Fallback to local call (Works in Console mode or if Service has access)
    match display::list_monitors() {
        Ok(monitors) => Json(monitors),
        Err(_) => Json(vec![]),
    }
}

#[derive(serde::Deserialize)]
struct TopologyRequest {
    mode: TopologyMode,
}

async fn set_topology_handler(Json(payload): Json<TopologyRequest>) -> Json<String> {
    // Attempt to set via Agent in Active Session
    // This is crucial for Service mode as SetDisplayConfig might fail in Session 0
    let mode_str = match &payload.mode {
        TopologyMode::Internal => "internal".to_string(),
        TopologyMode::External => "external".to_string(),
        TopologyMode::Extend => "extend".to_string(),
        TopologyMode::Clone => "clone".to_string(),
        TopologyMode::Single(id_str) => format!("single:{}", id_str),
    };

    if let Ok(output) = process_win::run_agent_in_active_session(&["set", &mode_str]) {
         // Agent usually prints "Success" or error
         return Json(output.trim().to_string());
    }

    // Fallback to local call
    match display::set_topology(payload.mode) {
        Ok(_) => Json("Success".to_string()),
        Err(e) => Json(format!("Error: {}", e)),
    }
}

async fn serve_assets(uri: axum::http::Uri) -> impl axum::response::IntoResponse {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    match Assets::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            ([(axum::http::header::CONTENT_TYPE, mime.as_ref())], content.data).into_response()
        }
        None => (axum::http::StatusCode::NOT_FOUND, "404 Not Found").into_response(),
    }
}
