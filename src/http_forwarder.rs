// Copyright (c) Azure Functions Web Adapter Contributors. All rights reserved.
// Licensed under the MIT License.

//! HTTP forwarder — converts Azure Functions `InvocationRequest` into HTTP
//! requests, forwards them to the user's web application, and converts the
//! HTTP response back into an `InvocationResponse`.
//!
//! This is the core translation layer, equivalent to Lambda Web Adapter's
//! event ↔ HTTP conversion.

use crate::config::AdapterConfig;
use crate::proto::{
    self, typed_data, InvocationRequest, InvocationResponse, ParameterBinding,
    RpcHttp, StatusResult, TypedData,
};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use std::collections::HashMap;
use tracing::{debug, error};

/// The HTTP forwarder that translates between gRPC invocations and HTTP.
pub struct HttpForwarder {
    config: AdapterConfig,
    client: Client<
        hyper_util::client::legacy::connect::HttpConnector,
        Full<Bytes>,
    >,
}

impl HttpForwarder {
    /// Create a new HTTP forwarder with the given configuration.
    pub fn new(config: &AdapterConfig) -> Self {
        let client = Client::builder(TokioExecutor::new())
            .build_http();

        Self {
            config: config.clone(),
            client,
        }
    }

    /// Forward an `InvocationRequest` to the user's web application and
    /// return an `InvocationResponse`.
    pub async fn forward(&self, request: &InvocationRequest) -> InvocationResponse {
        let invocation_id = request.invocation_id.clone();

        match self.do_forward(request).await {
            Ok(response) => response,
            Err(e) => {
                error!(
                    invocation_id = %invocation_id,
                    error = %e,
                    "failed to forward request"
                );
                InvocationResponse {
                    invocation_id,
                    result: Some(StatusResult {
                        status: proto::status_result::Status::Failure as i32,
                        result: format!("Web Adapter forwarding error: {}", e),
                        exception: None,
                        logs: vec![],
                    }),
                    output_data: vec![],
                    return_value: None,
                }
            }
        }
    }

    /// Internal forwarding logic.
    async fn do_forward(
        &self,
        invocation: &InvocationRequest,
    ) -> Result<InvocationResponse, Box<dyn std::error::Error + Send + Sync>> {
        // Extract the HTTP trigger data from input_data
        let rpc_http = self.extract_http_trigger(invocation)?;

        // Build the HTTP request
        let http_request = self.build_http_request(&rpc_http)?;

        debug!(
            method = %http_request.method(),
            uri = %http_request.uri(),
            invocation_id = %invocation.invocation_id,
            "forwarding request to web app"
        );

        // Send to the user's web application
        let response = self.client.request(http_request).await?;

        // Convert the HTTP response back to an InvocationResponse
        let invocation_response = self.build_invocation_response(
            &invocation.invocation_id,
            response,
        ).await?;

        Ok(invocation_response)
    }

    /// Extract the RpcHttp data from the invocation's trigger data or input bindings.
    fn extract_http_trigger(
        &self,
        invocation: &InvocationRequest,
    ) -> Result<RpcHttp, Box<dyn std::error::Error + Send + Sync>> {
        // First, check input_data for an HTTP binding (named "req" by convention)
        for binding in &invocation.input_data {
            if let Some(proto::parameter_binding::RpcData::Data(ref data)) = binding.rpc_data {
                if let Some(typed_data::Data::Http(ref http)) = data.data {
                    return Ok(*http.clone());
                }
            }
        }

        // Check trigger_metadata for "__request__" or "req"
        for key in &["req", "__request__", "Request"] {
            if let Some(data) = invocation.trigger_metadata.get(*key) {
                if let Some(typed_data::Data::Http(ref http)) = data.data {
                    return Ok(*http.clone());
                }
            }
        }

        Err("no HTTP trigger data found in invocation request".into())
    }

    /// Convert `RpcHttp` into a `hyper::Request`.
    fn build_http_request(
        &self,
        rpc_http: &RpcHttp,
    ) -> Result<hyper::Request<Full<Bytes>>, Box<dyn std::error::Error + Send + Sync>> {
        // Parse the URL from the RpcHttp
        let original_url: hyper::Uri = rpc_http.url.parse()?;

        // Extract the path and query, applying base path removal if configured
        let mut path = original_url.path().to_string();
        if let Some(ref base) = self.config.remove_base_path {
            if path.starts_with(base.as_str()) {
                path = path[base.len()..].to_string();
                if !path.starts_with('/') {
                    path = format!("/{}", path);
                }
            }
        }

        // Reconstruct the target URL pointing at the local web app
        let query = original_url.query().map(|q| format!("?{}", q)).unwrap_or_default();
        let target_url = format!("{}{}{}", self.config.app_base_url(), path, query);

        // Parse HTTP method
        let method: hyper::Method = rpc_http.method.to_uppercase().parse()?;

        // Build request
        let mut builder = hyper::Request::builder()
            .method(method)
            .uri(&target_url);

        // Copy headers
        for (key, value) in &rpc_http.headers {
            // Skip hop-by-hop headers and host (will be replaced)
            let lower_key = key.to_lowercase();
            if matches!(
                lower_key.as_str(),
                "host" | "transfer-encoding" | "connection" | "keep-alive"
            ) {
                continue;
            }
            builder = builder.header(key.as_str(), value.as_str());
        }

        // Also process nullable_headers
        for (key, nullable_val) in &rpc_http.nullable_headers {
            if let Some(proto::nullable_string::String::Value(ref v)) = nullable_val.string {
                let lower_key = key.to_lowercase();
                if !matches!(
                    lower_key.as_str(),
                    "host" | "transfer-encoding" | "connection" | "keep-alive"
                ) {
                    builder = builder.header(key.as_str(), v.as_str());
                }
            }
        }

        // Set the Host header for the local app
        builder = builder.header("Host", format!("{}:{}", self.config.host, self.config.port));

        // Extract body
        let body_bytes = self.extract_body(&rpc_http.body.as_deref());
        let body = Full::new(body_bytes);

        let request = builder.body(body)?;
        Ok(request)
    }

    /// Extract body bytes from TypedData.
    fn extract_body(&self, body: &Option<&TypedData>) -> Bytes {
        match body {
            Some(TypedData {
                data: Some(typed_data::Data::String(s)),
            }) => Bytes::from(s.clone()),
            Some(TypedData {
                data: Some(typed_data::Data::Json(s)),
            }) => Bytes::from(s.clone()),
            Some(TypedData {
                data: Some(typed_data::Data::Bytes(b)),
            }) => Bytes::from(b.clone()),
            Some(TypedData {
                data: Some(typed_data::Data::Stream(b)),
            }) => Bytes::from(b.clone()),
            _ => Bytes::new(),
        }
    }

    /// Convert a hyper HTTP response into an `InvocationResponse`.
    async fn build_invocation_response(
        &self,
        invocation_id: &str,
        response: hyper::Response<Incoming>,
    ) -> Result<InvocationResponse, Box<dyn std::error::Error + Send + Sync>> {
        let status = response.status().as_u16().to_string();

        // Collect response headers
        let mut headers: HashMap<String, String> = HashMap::new();
        for (key, value) in response.headers() {
            headers.insert(
                key.as_str().to_string(),
                value.to_str().unwrap_or("").to_string(),
            );
        }

        // Determine if response is binary
        let content_type = headers
            .get("content-type")
            .cloned()
            .unwrap_or_default();
        let is_binary = is_binary_content_type(&content_type);

        // Read the full response body
        let body_bytes = response.into_body().collect().await?.to_bytes();

        // Build the RpcHttp response
        let body_data = if is_binary {
            // Return as bytes for binary content
            Some(Box::new(TypedData {
                data: Some(typed_data::Data::Bytes(body_bytes.to_vec())),
            }))
        } else {
            // Return as string for text content
            Some(Box::new(TypedData {
                data: Some(typed_data::Data::String(
                    String::from_utf8_lossy(&body_bytes).to_string(),
                )),
            }))
        };

        let rpc_http_response = RpcHttp {
            status_code: status,
            headers,
            body: body_data,
            method: String::new(),
            url: String::new(),
            params: HashMap::new(),
            nullable_headers: HashMap::new(),
            nullable_params: HashMap::new(),
            nullable_query: HashMap::new(),
            query: HashMap::new(),
            enable_content_negotiation: false,
            raw_body: None,
            identities: vec![],
            cookies: vec![],
        };

        // For HTTP trigger functions, the host expects the response in return_value
        // as an RpcHttp-typed TypedData. We also put it in output_data["$return"]
        // for compatibility with both code paths in the host.
        let http_typed_data = TypedData {
            data: Some(typed_data::Data::Http(Box::new(rpc_http_response))),
        };

        let output_binding = ParameterBinding {
            name: "$return".to_string(),
            rpc_data: Some(proto::parameter_binding::RpcData::Data(http_typed_data.clone())),
        };

        debug!(
            invocation_id = %invocation_id,
            "forwarded response from web app"
        );

        Ok(InvocationResponse {
            invocation_id: invocation_id.to_string(),
            output_data: vec![output_binding],
            return_value: Some(http_typed_data),
            result: Some(StatusResult {
                status: proto::status_result::Status::Success as i32,
                result: String::new(),
                exception: None,
                logs: vec![],
            }),
        })
    }
}

/// Heuristic to determine if a content type represents binary data.
fn is_binary_content_type(ct: &str) -> bool {
    let ct_lower = ct.to_lowercase();
    if ct_lower.contains("text/")
        || ct_lower.contains("application/json")
        || ct_lower.contains("application/xml")
        || ct_lower.contains("application/javascript")
        || ct_lower.contains("application/x-www-form-urlencoded")
        || ct_lower.contains("+json")
        || ct_lower.contains("+xml")
    {
        return false;
    }
    if ct_lower.contains("application/octet-stream")
        || ct_lower.contains("image/")
        || ct_lower.contains("audio/")
        || ct_lower.contains("video/")
        || ct_lower.contains("application/pdf")
        || ct_lower.contains("application/zip")
        || ct_lower.contains("application/gzip")
    {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binary_content_type_detection() {
        assert!(!is_binary_content_type("text/html"));
        assert!(!is_binary_content_type("application/json"));
        assert!(!is_binary_content_type("application/xml; charset=utf-8"));
        assert!(is_binary_content_type("image/png"));
        assert!(is_binary_content_type("application/octet-stream"));
        assert!(is_binary_content_type("application/pdf"));
    }
}
