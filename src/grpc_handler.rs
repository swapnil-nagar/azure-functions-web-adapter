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
        let outbound = ReceiverStream::new(rx);

        // Start the bidirectional stream
        let response = client.event_stream(outbound).await?;
        let mut inbound = response.into_inner();

        // Send StartStream to establish our identity
        self.send_start_stream(&tx).await?;

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
                self.handle_worker_init(req, &request_id, tx).await?;
            }
            Some(streaming_message::Content::FunctionsMetadataRequest(req)) => {
                self.handle_functions_metadata(req, &request_id, tx).await?;
            }
            Some(streaming_message::Content::FunctionLoadRequest(req)) => {
                self.handle_function_load(req, &request_id, tx).await?;
            }
            Some(streaming_message::Content::InvocationRequest(req)) => {
                self.handle_invocation(req, &request_id, tx).await?;
            }
            Some(streaming_message::Content::WorkerStatusRequest(_)) => {
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
                debug!(?other, "unhandled message type");
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
        // Report capabilities similar to the Go Worker
        capabilities.insert("RpcHttpTriggerMetadataRemoved".to_string(), "true".to_string());
        capabilities.insert("RpcHttpBodyOnly".to_string(), "true".to_string());
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
        trigger_properties.insert("type".to_string(), "httpTrigger".to_string());
        trigger_properties.insert("direction".to_string(), "in".to_string());
        trigger_properties.insert("authLevel".to_string(), "anonymous".to_string());
        trigger_properties.insert("route".to_string(), "{*path}".to_string());

        let trigger_binding = BindingInfo {
            r#type: binding_info::BindingType::Trigger as i32,
            direction_string: "in".to_string(),
            data_type: "string".to_string(),
            properties: trigger_properties,
        };

        // HTTP output binding
        let mut output_properties = HashMap::new();
        output_properties.insert("type".to_string(), "http".to_string());
        output_properties.insert("direction".to_string(), "out".to_string());

        let output_binding = BindingInfo {
            r#type: binding_info::BindingType::Output as i32,
            direction_string: "out".to_string(),
            data_type: "string".to_string(),
            properties: output_properties,
        };

        let mut bindings = HashMap::new();
        bindings.insert("req".to_string(), trigger_binding);
        bindings.insert("$return".to_string(), output_binding);

        let function_metadata = RpcFunctionMetadata {
            name: self.function_name.clone(),
            directory: req.function_app_directory.clone(),
            script_file: String::new(),
            entry_point: String::new(),
            function_id: self.function_id.clone(),
            is_proxy: false,
            bindings,
            language: WORKER_LANGUAGE.to_string(),
            properties: HashMap::new(),
            retry_options: None,
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

    /// Handle InvocationRequest — the hot path!
    /// Forward the HTTP request to the user's web app and return the response.
    async fn handle_invocation(
        &self,
        req: InvocationRequest,
        request_id: &str,
        tx: &mpsc::Sender<StreamingMessage>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        debug!(
            invocation_id = %req.invocation_id,
            function_id = %req.function_id,
            "handling InvocationRequest"
        );

        // Forward to the user's web application
        let invocation_response = self.forwarder.forward(&req).await;

        let response = StreamingMessage {
            request_id: request_id.to_string(),
            content: Some(streaming_message::Content::InvocationResponse(
                invocation_response,
            )),
        };

        tx.send(response).await?;
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
                    result: Some(StatusResult {
                        status: status_result::Status::Success as i32,
                        result: String::new(),
                        exception: None,
                        logs: vec![],
                    }),
                    worker_init_response: None,
                },
            )),
        };

        tx.send(response).await?;
        debug!("sent FunctionEnvironmentReloadResponse");
        Ok(())
    }
}
