// Copyright (c) Azure Functions Web Adapter Contributors. All rights reserved.
// Licensed under the MIT License.

//! gRPC client for the Azure Functions Host.
//!
//! Implements the `FunctionRpc.EventStream` bidirectional streaming protocol.
//! This is the core integration layer that makes the web adapter a first-class
//! Azure Functions worker.

use crate::config::WorkerStartupArgs;
use crate::http_forwarder::HttpForwarder;
use crate::proto::{self, streaming_message, *};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::Channel;
use tracing::{debug, info};

/// Adapter capabilities reported to the Azure Functions Host.
const ADAPTER_VERSION: &str = "0.1.0";
const WORKER_LANGUAGE: &str = "web-adapter";

/// The gRPC message handler that bridges the Azure Functions Host
/// and the user's web application.
pub struct GrpcHandler {
    args: WorkerStartupArgs,
    forwarder: HttpForwarder,
    /// Registered function metadata (we register a single HTTP catch-all).
    function_id: String,
    function_name: String,
}

impl GrpcHandler {
    pub fn new(args: WorkerStartupArgs, forwarder: HttpForwarder) -> Self {
        let function_name = "WebAdapterHttpTrigger".to_string();
        // Generate a deterministic function ID by hashing the name
        let mut hasher = Sha256::new();
        hasher.update(function_name.as_bytes());
        let function_id = hex::encode(&hasher.finalize()[..16]);

        Self {
            args,
            forwarder,
            function_id,
            function_name,
        }
    }

    /// Connect to the Azure Functions Host and run the main event loop.
    ///
    /// This method does not return until the host terminates the worker
    /// or the connection is lost.
    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!(
            uri = %self.args.functions_uri,
            worker_id = %self.args.worker_id,
            "connecting to Azure Functions Host"
        );

        // Connect to the host's gRPC server
        let channel = Channel::from_shared(self.args.functions_uri.clone())?
            .connect()
            .await?;

        let mut client = proto::function_rpc_client::FunctionRpcClient::new(channel)
            .max_decoding_message_size(self.args.grpc_max_message_length)
            .max_encoding_message_size(self.args.grpc_max_message_length);

        // Create a channel for sending messages to the host
        let (tx, rx) = mpsc::channel::<StreamingMessage>(256);

        // CRITICAL: Send StartStream BEFORE calling event_stream().
        // The Azure Functions Host doesn't send response HEADERS until it
        // receives StartStream. If we send StartStream after event_stream(),
        // we get a deadlock (client waits for HEADERS, host waits for StartStream).
        self.send_start_stream(&tx).await?;

        let outbound = ReceiverStream::new(rx);

        // Start the bidirectional stream — host sees StartStream immediately
        let response = client.event_stream(outbound).await?;
        let mut inbound = response.into_inner();

        info!("gRPC event stream established, entering message loop");

        // Main message loop
        while let Some(msg) = inbound.message().await? {
            self.handle_message(msg, &tx).await?;
        }

        info!("gRPC stream closed by host");
        Ok(())
    }

    /// Send the initial StartStream message.
    async fn send_start_stream(
        &self,
        tx: &mpsc::Sender<StreamingMessage>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let msg = StreamingMessage {
            request_id: self.args.request_id.clone(),
            content: Some(streaming_message::Content::StartStream(StartStream {
                worker_id: self.args.worker_id.clone(),
            })),
        };
        tx.send(msg).await?;
        debug!("sent StartStream");
        Ok(())
    }

    /// Dispatch an incoming message from the host.
    async fn handle_message(
        &self,
        msg: StreamingMessage,
        tx: &mpsc::Sender<StreamingMessage>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let request_id = msg.request_id.clone();

        match msg.content {
            Some(streaming_message::Content::WorkerInitRequest(req)) => {
                info!(request_id = %request_id, "received WorkerInitRequest");
                self.handle_worker_init(req, &request_id, tx).await?;
            }
            Some(streaming_message::Content::FunctionsMetadataRequest(req)) => {
                info!(request_id = %request_id, "received FunctionsMetadataRequest");
                self.handle_functions_metadata(req, &request_id, tx).await?;
            }
            Some(streaming_message::Content::FunctionLoadRequest(req)) => {
                info!(request_id = %request_id, "received FunctionLoadRequest");
                self.handle_function_load(req, &request_id, tx).await?;
            }
            Some(streaming_message::Content::FunctionLoadRequestCollection(req)) => {
                info!(request_id = %request_id, count = req.function_load_requests.len(), "received FunctionLoadRequestCollection");
                self.handle_function_load_collection(req, &request_id, tx).await?;
            }
            Some(streaming_message::Content::InvocationRequest(req)) => {
                info!(request_id = %request_id, invocation_id = %req.invocation_id, "received InvocationRequest");
                self.handle_invocation(req, &request_id, tx).await?;
            }
            Some(streaming_message::Content::WorkerStatusRequest(_)) => {
                info!(request_id = %request_id, "received WorkerStatusRequest");
                self.handle_worker_status(&request_id, tx).await?;
            }
            Some(streaming_message::Content::WorkerHeartbeat(_)) => {
                self.handle_heartbeat(&request_id, tx).await?;
            }
            Some(streaming_message::Content::FunctionEnvironmentReloadRequest(req)) => {
                self.handle_env_reload(req, &request_id, tx).await?;
            }
            Some(streaming_message::Content::WorkerTerminate(_)) => {
                info!("received WorkerTerminate from host");
                return Err("worker terminated by host".into());
            }
            other => {
                info!(?other, "unhandled message type");
            }
        }

        Ok(())
    }

    /// Handle WorkerInitRequest — report our capabilities.
    async fn handle_worker_init(
        &self,
        req: WorkerInitRequest,
        request_id: &str,
        tx: &mpsc::Sender<StreamingMessage>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!(
            host_version = %req.host_version,
            app_dir = %req.function_app_directory,
            "handling WorkerInitRequest"
        );

        let mut capabilities = HashMap::new();
        // Report capabilities to the host.
        // NOTE: Do NOT set RpcHttpBodyOnly or RpcHttpTriggerMetadataRemoved —
        // we need the full RpcHttp object to forward requests to the web app.
        capabilities.insert("HandlesWorkerTerminateMessage".to_string(), "true".to_string());
        capabilities.insert("SupportsLoadResponseCollection".to_string(), "true".to_string());
        capabilities.insert("WorkerStatus".to_string(), "true".to_string());

        let response = StreamingMessage {
            request_id: request_id.to_string(),
            content: Some(streaming_message::Content::WorkerInitResponse(
                WorkerInitResponse {
                    worker_version: ADAPTER_VERSION.to_string(),
                    capabilities,
                    result: Some(StatusResult {
                        status: status_result::Status::Success as i32,
                        result: String::new(),
                        exception: None,
                        logs: vec![],
                    }),
                    worker_metadata: Some(WorkerMetadata {
                        runtime_name: "web-adapter".to_string(),
                        runtime_version: ADAPTER_VERSION.to_string(),
                        worker_version: ADAPTER_VERSION.to_string(),
                        worker_bitness: std::env::consts::ARCH.to_string(),
                        custom_properties: HashMap::new(),
                    }),
                },
            )),
        };

        tx.send(response).await?;
        debug!("sent WorkerInitResponse");
        Ok(())
    }

    /// Handle FunctionsMetadataRequest — register our HTTP catch-all function.
    async fn handle_functions_metadata(
        &self,
        req: FunctionsMetadataRequest,
        request_id: &str,
        tx: &mpsc::Sender<StreamingMessage>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!(
            app_dir = %req.function_app_directory,
            "handling FunctionsMetadataRequest — registering HTTP catch-all"
        );

        // Build the HTTP trigger binding — catches all routes and methods
        let mut trigger_properties = HashMap::new();
        trigger_properties.insert("authLevel".to_string(), "anonymous".to_string());
        trigger_properties.insert("route".to_string(), "{*path}".to_string());

        let trigger_binding = BindingInfo {
            r#type: "httpTrigger".to_string(),
            direction: binding_info::Direction::In as i32,
            data_type: binding_info::DataType::String as i32,
            properties: trigger_properties,
        };

        // HTTP output binding
        let output_binding = BindingInfo {
            r#type: "http".to_string(),
            direction: binding_info::Direction::Out as i32,
            data_type: binding_info::DataType::Undefined as i32,
            properties: HashMap::new(),
        };

        let mut bindings = HashMap::new();
        bindings.insert("req".to_string(), trigger_binding);
        bindings.insert("$return".to_string(), output_binding);

        // raw_bindings: JSON-serialized binding definitions that the host parses
        let raw_bindings = vec![
            serde_json::json!({
                "name": "req",
                "type": "httpTrigger",
                "direction": "in",
                "authLevel": "anonymous",
                "route": "{*path}",
                "methods": ["get", "post", "put", "delete", "patch", "head", "options"]
            }).to_string(),
            serde_json::json!({
                "name": "$return",
                "type": "http",
                "direction": "out"
            }).to_string(),
        ];

        // script_file must point to an actual file for the host to accept.
        // Use the current executable path (the adapter binary itself).
        let script_file = std::env::current_exe()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "azure-func-web-adapter".to_string());

        let function_metadata = RpcFunctionMetadata {
            name: self.function_name.clone(),
            directory: req.function_app_directory.clone(),
            script_file,
            entry_point: self.function_name.clone(),
            function_id: self.function_id.clone(),
            is_proxy: false,
            bindings,
            status: None,
            language: WORKER_LANGUAGE.to_string(),
            raw_bindings,
            managed_dependency_enabled: false,
            retry_options: None,
            properties: HashMap::new(),
        };

        let response = StreamingMessage {
            request_id: request_id.to_string(),
            content: Some(streaming_message::Content::FunctionMetadataResponse(
                FunctionMetadataResponse {
                    function_metadata_results: vec![function_metadata],
                    result: Some(StatusResult {
                        status: status_result::Status::Success as i32,
                        result: String::new(),
                        exception: None,
                        logs: vec![],
                    }),
                    use_default_metadata_indexing: false,
                },
            )),
        };

        tx.send(response).await?;
        debug!("sent FunctionMetadataResponse with HTTP catch-all");
        Ok(())
    }

    /// Handle FunctionLoadRequest — acknowledge function loading.
    async fn handle_function_load(
        &self,
        req: FunctionLoadRequest,
        request_id: &str,
        tx: &mpsc::Sender<StreamingMessage>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!(
            function_id = %req.function_id,
            "handling FunctionLoadRequest"
        );

        let response = StreamingMessage {
            request_id: request_id.to_string(),
            content: Some(streaming_message::Content::FunctionLoadResponse(
                FunctionLoadResponse {
                    function_id: req.function_id,
                    result: Some(StatusResult {
                        status: status_result::Status::Success as i32,
                        result: String::new(),
                        exception: None,
                        logs: vec![],
                    }),
                    is_dependency_downloaded: false,
                },
            )),
        };

        tx.send(response).await?;
        debug!("sent FunctionLoadResponse");
        Ok(())
    }

    /// Handle FunctionLoadRequestCollection — batch acknowledge function loading.
    async fn handle_function_load_collection(
        &self,
        req: FunctionLoadRequestCollection,
        request_id: &str,
        tx: &mpsc::Sender<StreamingMessage>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let responses: Vec<FunctionLoadResponse> = req
            .function_load_requests
            .iter()
            .map(|load_req| {
                info!(function_id = %load_req.function_id, "loading function (batch)");
                FunctionLoadResponse {
                    function_id: load_req.function_id.clone(),
                    result: Some(StatusResult {
                        status: status_result::Status::Success as i32,
                        result: String::new(),
                        exception: None,
                        logs: vec![],
                    }),
                    is_dependency_downloaded: false,
                }
            })
            .collect();

        let response = StreamingMessage {
            request_id: request_id.to_string(),
            content: Some(streaming_message::Content::FunctionLoadResponseCollection(
                FunctionLoadResponseCollection {
                    function_load_responses: responses,
                },
            )),
        };

        tx.send(response).await?;
        info!("sent FunctionLoadResponseCollection");
        Ok(())
    }

    /// Handle InvocationRequest — the hot path!
    /// Forward the HTTP request to the user's web app and return the response.
    async fn handle_invocation(
        &self,
        req: InvocationRequest,
        request_id: &str,
        tx: &mpsc::Sender<StreamingMessage>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!(
            invocation_id = %req.invocation_id,
            function_id = %req.function_id,
            input_count = req.input_data.len(),
            trigger_keys = ?req.trigger_metadata.keys().collect::<Vec<_>>(),
            "handling InvocationRequest"
        );

        // Forward to the user's web application
        let invocation_response = self.forwarder.forward(&req).await;

        info!(
            invocation_id = %req.invocation_id,
            status = ?invocation_response.result.as_ref().map(|r| r.status),
            "sending InvocationResponse"
        );

        let response = StreamingMessage {
            request_id: request_id.to_string(),
            content: Some(streaming_message::Content::InvocationResponse(
                invocation_response,
            )),
        };

        tx.send(response).await?;
        info!("InvocationResponse sent to channel");
        Ok(())
    }

    /// Handle WorkerStatusRequest — respond with empty status (healthy).
    async fn handle_worker_status(
        &self,
        request_id: &str,
        tx: &mpsc::Sender<StreamingMessage>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let response = StreamingMessage {
            request_id: request_id.to_string(),
            content: Some(streaming_message::Content::WorkerStatusResponse(
                WorkerStatusResponse {},
            )),
        };
        tx.send(response).await?;
        Ok(())
    }

    /// Handle WorkerHeartbeat — echo back.
    async fn handle_heartbeat(
        &self,
        request_id: &str,
        tx: &mpsc::Sender<StreamingMessage>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let response = StreamingMessage {
            request_id: request_id.to_string(),
            content: Some(streaming_message::Content::WorkerHeartbeat(
                WorkerHeartbeat {},
            )),
        };
        tx.send(response).await?;
        Ok(())
    }

    /// Handle FunctionEnvironmentReloadRequest (specialization in placeholder mode).
    async fn handle_env_reload(
        &self,
        req: FunctionEnvironmentReloadRequest,
        request_id: &str,
        tx: &mpsc::Sender<StreamingMessage>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!(
            num_vars = req.environment_variables.len(),
            app_dir = %req.function_app_directory,
            "handling FunctionEnvironmentReloadRequest (specialization)"
        );

        // Apply new environment variables
        for (key, value) in &req.environment_variables {
            std::env::set_var(key, value);
        }

        let response = StreamingMessage {
            request_id: request_id.to_string(),
            content: Some(streaming_message::Content::FunctionEnvironmentReloadResponse(
                FunctionEnvironmentReloadResponse {
                    worker_metadata: None,
                    capabilities: HashMap::new(),
                    result: Some(StatusResult {
                        status: status_result::Status::Success as i32,
                        result: String::new(),
                        exception: None,
                        logs: vec![],
                    }),
                    capabilities_update_strategy: 0,
                },
            )),
        };

        tx.send(response).await?;
        debug!("sent FunctionEnvironmentReloadResponse");
        Ok(())
    }
}
