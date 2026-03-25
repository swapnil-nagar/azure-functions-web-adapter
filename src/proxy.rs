// Copyright (c) Azure Functions Web Adapter Contributors. All rights reserved.
// Licensed under the MIT License.

//! Proxy mode — implements the Go Worker's Proxy Model for placeholder /
//! consumption plan deployments.
//!
//! In placeholder mode, the proxy:
//! 1. Starts first and connects to the Azure Functions Host via gRPC.
//! 2. Handles init messages with stub responses (no user code loaded yet).
//! 3. On specialization (`FunctionEnvironmentReloadRequest`), spawns the
//!    user's web application and the real web adapter worker.
//! 4. Bridges gRPC messages between the host and the child worker.
//!
//! This enables fixed base images where the proxy is pre-installed, and user
//! code is loaded dynamically at specialization time.

use crate::config::WorkerStartupArgs;
use crate::proto::{self, streaming_message, *};
use std::collections::HashMap;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::Channel;
use tracing::{debug, error, info, warn};

const PROXY_VERSION: &str = "0.1.0";

/// Run in proxy/placeholder mode.
///
/// * If `WEBSITE_PLACEHOLDER_MODE=1`, wait for specialization before spawning child.
/// * Otherwise, spawn child immediately (dedicated mode via proxy).
pub async fn run_proxy(args: WorkerStartupArgs) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let is_placeholder = std::env::var("WEBSITE_PLACEHOLDER_MODE")
        .map(|v| v == "1")
        .unwrap_or(false);

    info!(
        is_placeholder,
        "starting in proxy mode"
    );

    // Connect to the host's gRPC server
    let channel = Channel::from_shared(args.functions_uri.clone())?
        .connect()
        .await?;

    let mut client = proto::function_rpc_client::FunctionRpcClient::new(channel)
        .max_decoding_message_size(args.grpc_max_message_length)
        .max_encoding_message_size(args.grpc_max_message_length);

    // Create channel for sending messages to the host
    let (host_tx, host_rx) = mpsc::channel::<StreamingMessage>(256);
    let outbound = ReceiverStream::new(host_rx);

    // Start the bidirectional stream with the host
    let response = client.event_stream(outbound).await?;
    let mut host_inbound = response.into_inner();

    // Send StartStream
    host_tx
        .send(StreamingMessage {
            request_id: args.request_id.clone(),
            content: Some(streaming_message::Content::StartStream(StartStream {
                worker_id: args.worker_id.clone(),
            })),
        })
        .await?;

    info!("proxy connected to host, entering message loop");

    // State for the proxy
    let mut saved_init_request: Option<(String, WorkerInitRequest)> = None;
    let mut child_tx: Option<mpsc::Sender<StreamingMessage>> = None;
    let mut specialized = false;

    // If not in placeholder mode, immediately start specialization
    if !is_placeholder {
        info!("not in placeholder mode, will spawn child on first init");
    }

    // Main message loop — process messages from the host
    while let Some(msg) = host_inbound.message().await? {
        let request_id = msg.request_id.clone();

        match &msg.content {
            // --- WorkerInitRequest ---
            Some(streaming_message::Content::WorkerInitRequest(req)) => {
                info!(
                    host_version = %req.host_version,
                    "proxy handling WorkerInitRequest"
                );

                // Save for replay to child later
                saved_init_request = Some((request_id.clone(), req.clone()));

                if is_placeholder || child_tx.is_none() {
                    // Respond with stub capabilities
                    let mut capabilities = HashMap::new();
                    capabilities.insert(
                        "RpcHttpTriggerMetadataRemoved".to_string(),
                        "true".to_string(),
                    );
                    capabilities.insert("RpcHttpBodyOnly".to_string(), "true".to_string());
                    capabilities.insert(
                        "HandlesWorkerTerminateMessage".to_string(),
                        "true".to_string(),
                    );
                    capabilities.insert("WorkerStatus".to_string(), "true".to_string());

                    host_tx
                        .send(StreamingMessage {
                            request_id,
                            content: Some(streaming_message::Content::WorkerInitResponse(
                                WorkerInitResponse {
                                    worker_version: PROXY_VERSION.to_string(),
                                    capabilities,
                                    result: Some(StatusResult {
                                        status: status_result::Status::Success as i32,
                                        result: String::new(),
                                        exception: None,
                                        logs: vec![],
                                    }),
                                },
                            )),
                        })
                        .await?;

                    // In non-placeholder mode, spawn the child now
                    if !is_placeholder && child_tx.is_none() {
                        child_tx = Some(
                            spawn_child_adapter(&args, &saved_init_request, &host_tx).await?,
                        );
                        specialized = true;
                    }
                } else if let Some(ref ctx) = child_tx {
                    // Forward to child
                    ctx.send(msg.clone()).await?;
                }
            }

            // --- FunctionsMetadataRequest ---
            Some(streaming_message::Content::FunctionsMetadataRequest(_)) => {
                if let Some(ref ctx) = child_tx {
                    // Forward to child
                    ctx.send(msg).await?;
                } else {
                    // Placeholder mode: return empty function list
                    host_tx
                        .send(StreamingMessage {
                            request_id,
                            content: Some(streaming_message::Content::FunctionMetadataResponse(
                                FunctionMetadataResponse {
                                    function_metadata_results: vec![],
                                    result: Some(StatusResult {
                                        status: status_result::Status::Success as i32,
                                        result: String::new(),
                                        exception: None,
                                        logs: vec![],
                                    }),
                                    use_default_metadata_indexing: false,
                                },
                            )),
                        })
                        .await?;
                }
            }

            // --- FunctionEnvironmentReloadRequest (SPECIALIZATION) ---
            Some(streaming_message::Content::FunctionEnvironmentReloadRequest(req)) => {
                if specialized {
                    warn!("already specialized, ignoring duplicate specialization");
                    continue;
                }

                info!(
                    app_dir = %req.function_app_directory,
                    num_vars = req.environment_variables.len(),
                    "SPECIALIZATION: spawning child adapter"
                );

                // Apply environment variables
                for (key, value) in &req.environment_variables {
                    std::env::set_var(key, value);
                }

                // Spawn the child web adapter
                child_tx = Some(
                    spawn_child_adapter(&args, &saved_init_request, &host_tx).await?,
                );
                specialized = true;

                // Respond to the host
                host_tx
                    .send(StreamingMessage {
                        request_id,
                        content: Some(
                            streaming_message::Content::FunctionEnvironmentReloadResponse(
                                FunctionEnvironmentReloadResponse {
                                    result: Some(StatusResult {
                                        status: status_result::Status::Success as i32,
                                        result: String::new(),
                                        exception: None,
                                        logs: vec![],
                                    }),
                                    worker_init_response: None,
                                },
                            ),
                        ),
                    })
                    .await?;
            }

            // --- WorkerHeartbeat ---
            Some(streaming_message::Content::WorkerHeartbeat(_)) => {
                host_tx
                    .send(StreamingMessage {
                        request_id,
                        content: Some(streaming_message::Content::WorkerHeartbeat(
                            WorkerHeartbeat {},
                        )),
                    })
                    .await?;
            }

            // --- WorkerStatusRequest ---
            Some(streaming_message::Content::WorkerStatusRequest(_)) => {
                if let Some(ref ctx) = child_tx {
                    ctx.send(msg).await?;
                } else {
                    host_tx
                        .send(StreamingMessage {
                            request_id,
                            content: Some(streaming_message::Content::WorkerStatusResponse(
                                WorkerStatusResponse {},
                            )),
                        })
                        .await?;
                }
            }

            // --- WorkerTerminate ---
            Some(streaming_message::Content::WorkerTerminate(_)) => {
                info!("proxy received WorkerTerminate");
                if let Some(ref ctx) = child_tx {
                    let _ = ctx.send(msg).await;
                }
                break;
            }

            // --- All other messages: forward to child if connected ---
            _ => {
                if let Some(ref ctx) = child_tx {
                    ctx.send(msg).await?;
                } else {
                    debug!("message received before specialization, dropping");
                }
            }
        }
    }

    info!("proxy shutting down");
    Ok(())
}

/// Spawn the child web adapter process.
///
/// In production, this would spawn a separate process. For simplicity in this
/// implementation, we spawn a new async task that runs the full adapter pipeline
/// (web app process + readiness check + gRPC handler) and communicate via channels.
async fn spawn_child_adapter(
    args: &WorkerStartupArgs,
    saved_init: &Option<(String, WorkerInitRequest)>,
    host_tx: &mpsc::Sender<StreamingMessage>,
) -> Result<mpsc::Sender<StreamingMessage>, Box<dyn std::error::Error + Send + Sync>> {
    // Create a channel for sending host messages to the child task
    let (child_tx, mut child_rx) = mpsc::channel::<StreamingMessage>(256);
    let host_tx = host_tx.clone();
    let _args = args.clone();
    let _saved_init = saved_init.clone();

    tokio::spawn(async move {
        info!("child adapter task started");

        // In a full implementation, this would:
        // 1. Spawn the user's web app process
        // 2. Wait for readiness
        // 3. Start processing invocation requests from the channel
        // 4. Forward responses back to host_tx

        // For now, we process messages from the channel and forward responses
        // This demonstrates the bridging pattern from the Go Worker spec.

        use crate::config::AdapterConfig;
        use crate::http_forwarder::HttpForwarder;
        use crate::process::ProcessManager;
        use crate::readiness;

        let config = AdapterConfig::from_env();
        let forwarder = HttpForwarder::new(&config);

        // Spawn the web app if a startup command is configured
        let mut process_mgr = ProcessManager::new();
        if let Some(ref cmd) = config.startup_command {
            let env_vars: HashMap<String, String> = HashMap::new();
            if let Err(e) = process_mgr.spawn(cmd, None, env_vars).await {
                error!(error = %e, "failed to spawn web application");
                return;
            }

            // Wait for readiness
            match readiness::wait_until_ready(&config).await {
                Ok(elapsed) => info!(?elapsed, "web application ready (child adapter)"),
                Err(e) => {
                    error!(error = %e, "web application readiness check failed");
                    process_mgr.shutdown().await;
                    return;
                }
            }
        }

        // Process messages from the proxy
        while let Some(msg) = child_rx.recv().await {
            let request_id = msg.request_id.clone();

            match msg.content {
                Some(streaming_message::Content::InvocationRequest(req)) => {
                    let response = forwarder.forward(&req).await;
                    let _ = host_tx
                        .send(StreamingMessage {
                            request_id,
                            content: Some(streaming_message::Content::InvocationResponse(response)),
                        })
                        .await;
                }
                Some(streaming_message::Content::WorkerTerminate(_)) => {
                    info!("child adapter received terminate");
                    process_mgr.shutdown().await;
                    break;
                }
                Some(streaming_message::Content::WorkerStatusRequest(_)) => {
                    let _ = host_tx
                        .send(StreamingMessage {
                            request_id,
                            content: Some(streaming_message::Content::WorkerStatusResponse(
                                WorkerStatusResponse {},
                            )),
                        })
                        .await;
                }
                _ => {
                    debug!("child adapter: unhandled message");
                }
            }
        }

        // Clean up
        process_mgr.shutdown().await;
        info!("child adapter task exiting");
    });

    Ok(child_tx)
}
