// Copyright (c) Azure Functions Web Adapter Contributors. All rights reserved.
// Licensed under the MIT License.

//! Azure Functions Web Adapter — binary entry point.
//!
//! Supports three modes:
//!
//! 1. **Custom Handler mode** (`func start` / local development):
//!    Activated when `FUNCTIONS_CUSTOMHANDLER_PORT` is set. The adapter runs
//!    as an HTTP reverse proxy, forwarding requests from the Functions Host
//!    to the user's web application.
//!
//! 2. **Direct gRPC mode** (production with language worker):
//!    Activated with `--host`/`--port` or `--functions-uri` CLI flags.
//!    The adapter connects to the Functions Host via gRPC and handles
//!    invocations.
//!
//! 3. **Proxy/placeholder mode** (`AZURE_FWA_MODE=proxy`):
//!    A lightweight proxy for consumption plan pre-warm scenarios.

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

    // --- Custom Handler mode (func start / local development) ---
    // Activated when FUNCTIONS_CUSTOMHANDLER_PORT is set by the Functions Host.
    if let Ok(handler_port_str) = std::env::var("FUNCTIONS_CUSTOMHANDLER_PORT") {
        if let Ok(handler_port) = handler_port_str.parse::<u16>() {
            info!(
                handler_port = handler_port,
                "running in CUSTOM HANDLER mode (func start)"
            );
            return run_custom_handler_mode(handler_port).await;
        }
    }

    // Parse startup arguments from CLI flags (needed for gRPC and proxy modes)
    let args = WorkerStartupArgs::from_args().map_err(|e| {
        error!(error = %e, "failed to parse startup arguments");
        e
    })?;

    // Check if we should run in proxy mode
    let mode = std::env::var("AZURE_FWA_MODE").unwrap_or_default();
    if mode.to_lowercase() == "proxy" {
        info!("running in PROXY mode");
        return proxy::run_proxy(args).await;
    }

    // --- Direct gRPC mode ---
    info!("running in DIRECT gRPC mode");
    run_grpc_mode(args).await
}

/// Custom Handler mode: HTTP reverse proxy for `func start`.
async fn run_custom_handler_mode(handler_port: u16) -> Result<(), Error> {
    let config = AdapterConfig::from_env();

    info!(
        app_port = config.port,
        host = %config.host,
        readiness_path = %config.readiness_check_path,
        "adapter configuration loaded"
    );

    // Spawn the user's web application
    let mut process_mgr = ProcessManager::new();
    if let Some(ref cmd) = config.startup_command {
        let env_vars: HashMap<String, String> = HashMap::new();
        process_mgr.spawn(cmd, None, env_vars).await?;
    } else {
        info!("no startup command configured, assuming app is already running");
    }

    // Wait for the web application to become ready
    match readiness::wait_until_ready(&config).await {
        Ok(elapsed) => {
            info!(?elapsed, "web application is ready, starting HTTP proxy");
        }
        Err(e) => {
            error!(error = %e, "web application failed readiness check");
            process_mgr.shutdown().await;
            return Err(e.into());
        }
    }

    // Run the HTTP reverse proxy
    let target_url = config.app_base_url();
    let result = http_proxy::run_http_proxy(handler_port, target_url).await;

    process_mgr.shutdown().await;
    result
}

/// Direct gRPC mode: connect to Functions Host via gRPC.
async fn run_grpc_mode(args: WorkerStartupArgs) -> Result<(), Error> {
    let config = AdapterConfig::from_env();

    info!(
        port = config.port,
        host = %config.host,
        readiness_path = %config.readiness_check_path,
        "adapter configuration loaded"
    );

    // Spawn the user's web application if a startup command is configured
    let mut process_mgr = ProcessManager::new();
    if let Some(ref cmd) = config.startup_command {
        let env_vars: HashMap<String, String> = HashMap::new();
        process_mgr.spawn(cmd, None, env_vars).await?;
    } else {
        info!("no startup command configured (AZURE_FWA_STARTUP_COMMAND), assuming app is already running");
    }

    // Wait for the web application to become ready
    match readiness::wait_until_ready(&config).await {
        Ok(elapsed) => {
            info!(?elapsed, "web application is ready, starting gRPC handler");
        }
        Err(e) => {
            error!(error = %e, "web application failed readiness check");
            process_mgr.shutdown().await;
            return Err(e.into());
        }
    }

    // Create the HTTP forwarder
    let forwarder = HttpForwarder::new(&config);

    // Create and run the gRPC handler
    let handler = GrpcHandler::new(args, forwarder);

    // Run until the host terminates us
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
