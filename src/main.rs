// Copyright (c) Azure Functions Web Adapter Contributors. All rights reserved.
// Licensed under the MIT License.

//! Azure Functions Web Adapter — binary entry point.
//!
//! Auto-detects the run mode based on environment:
//!
//! 1. **gRPC language worker mode** (Docker / production):
//!    When launched with `--host/--port/--workerId` CLI args by the Azure
//!    Functions Host. The adapter connects via gRPC, registers functions
//!    dynamically, and translates InvocationRequests to HTTP.
//!    **No function.json, no customHandler needed.**
//!
//! 2. **HTTP proxy mode** (`func start` / local development):
//!    When `FUNCTIONS_CUSTOMHANDLER_PORT` is set by the host. The adapter
//!    runs as an HTTP reverse proxy, forwarding requests to your web app.
//!    Pair with `routePrefix: ""` in host.json for clean path mapping.
//!
//! 3. **Proxy/placeholder mode** (consumption plan):
//!    When `AZURE_FWA_MODE=proxy`. Handles pre-warm before specialization.
//!
//! In ALL modes, your web app needs ZERO code changes.

use azure_functions_web_adapter::{
    config::{AdapterConfig, WorkerStartupArgs},
    grpc_handler::GrpcHandler,
    http_forwarder::HttpForwarder,
    http_proxy,
    process::ProcessManager,
    proxy, readiness, Error,
};
use std::collections::HashMap;
use tracing::{error, info};

#[tokio::main]
async fn main() -> Result<(), Error> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("AZURE_FWA_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    info!(
        version = env!("CARGO_PKG_VERSION"),
        "Azure Functions Web Adapter starting"
    );

    // --- Mode detection ---

    // 1. HTTP proxy mode: func start sets FUNCTIONS_CUSTOMHANDLER_PORT
    if let Ok(handler_port_str) = std::env::var("FUNCTIONS_CUSTOMHANDLER_PORT") {
        if let Ok(handler_port) = handler_port_str.parse::<u16>() {
            info!(handler_port, "running in HTTP proxy mode (func start)");
            return run_http_proxy_mode(handler_port).await;
        }
    }

    // 2. gRPC / proxy mode: parse CLI args from the Functions Host
    let args = WorkerStartupArgs::from_args().map_err(|e| {
        error!(error = %e, "failed to parse startup arguments — this binary must be launched by the Azure Functions Host");
        e
    })?;

    // 3. Proxy/placeholder mode for consumption plan
    let mode = std::env::var("AZURE_FWA_MODE").unwrap_or_default();
    if mode.to_lowercase() == "proxy" {
        info!("running in proxy mode (placeholder / consumption plan)");
        return proxy::run_proxy(args).await;
    }

    // 4. gRPC language worker mode (primary production path)
    info!("running as gRPC language worker");
    run_grpc_worker(args).await
}

/// HTTP proxy mode for `func start` local development.
///
/// The host sets FUNCTIONS_CUSTOMHANDLER_PORT and the adapter listens
/// on that port, proxying requests to the user's web app.
///
/// Pair with `"routePrefix": ""` in host.json and a catch-all function.json
/// so routes map 1:1 to the web app.
async fn run_http_proxy_mode(handler_port: u16) -> Result<(), Error> {
    let config = AdapterConfig::from_env();

    info!(
        app_port = config.port,
        host = %config.host,
        readiness_path = %config.readiness_check_path,
        remove_base_path = ?config.remove_base_path,
        "adapter configuration loaded"
    );

    // Spawn the user's web application
    let mut process_mgr = ProcessManager::new();
    if let Some(ref cmd) = config.startup_command {
        let env_vars: HashMap<String, String> = HashMap::new();
        process_mgr.spawn(cmd, None, env_vars).await?;
    } else {
        info!("no AZURE_FWA_STARTUP_COMMAND set, assuming web app is already running");
    }

    // Wait for the web application to become ready
    match readiness::wait_until_ready(&config).await {
        Ok(elapsed) => {
            info!(?elapsed, "web application is ready");
        }
        Err(e) => {
            error!(error = %e, "web application failed readiness check");
            process_mgr.shutdown().await;
            return Err(e.into());
        }
    }

    // Run the HTTP reverse proxy
    let target_url = config.app_base_url();
    let remove_base_path = config.remove_base_path.clone();
    let result = http_proxy::run_http_proxy(handler_port, target_url, remove_base_path).await;

    process_mgr.shutdown().await;
    result
}

/// gRPC language worker mode for Docker / production deployment.
///
/// Connects to the Azure Functions Host via gRPC, dynamically registers
/// an HTTP catch-all function, and translates InvocationRequests to HTTP.
/// **No function.json or customHandler needed.**
async fn run_grpc_worker(args: WorkerStartupArgs) -> Result<(), Error> {
    let config = AdapterConfig::from_env();

    info!(
        app_port = config.port,
        host = %config.host,
        readiness_path = %config.readiness_check_path,
        startup_command = ?config.startup_command,
        "adapter configuration loaded"
    );

    // Spawn the user's web application
    let mut process_mgr = ProcessManager::new();
    if let Some(ref cmd) = config.startup_command {
        let env_vars: HashMap<String, String> = HashMap::new();
        process_mgr.spawn(cmd, None, env_vars).await?;
    } else {
        info!("no AZURE_FWA_STARTUP_COMMAND set, assuming web app is already running");
    }

    // Wait for the web application to become ready
    match readiness::wait_until_ready(&config).await {
        Ok(elapsed) => {
            info!(?elapsed, "web application is ready");
        }
        Err(e) => {
            error!(error = %e, "web application failed readiness check");
            process_mgr.shutdown().await;
            return Err(e.into());
        }
    }

    // Create the HTTP forwarder (translates gRPC InvocationRequest → HTTP → InvocationResponse)
    let forwarder = HttpForwarder::new(&config);

    // Create and run the gRPC handler
    let handler = GrpcHandler::new(args, forwarder);
    let result = handler.run().await;

    // Graceful shutdown
    info!("shutting down web application");
    process_mgr.shutdown().await;

    match result {
        Ok(()) => {
            info!("adapter exited cleanly");
            Ok(())
        }
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("worker terminated by host") {
                info!("adapter terminated by host (normal shutdown)");
                Ok(())
            } else {
                error!(error = %e, "adapter exited with error");
                Err(e)
            }
        }
    }
}
