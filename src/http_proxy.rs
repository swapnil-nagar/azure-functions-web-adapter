// Copyright (c) Azure Functions Web Adapter Contributors. All rights reserved.
// Licensed under the MIT License.

//! HTTP reverse proxy for Azure Functions Custom Handler mode.
//!
//! When running under `func start` (local development), the Azure Functions Host
//! uses the Custom Handler protocol: it sends HTTP requests to the adapter, and
//! the adapter forwards them to the user's web application.
//!
//! This mode is activated when `FUNCTIONS_CUSTOMHANDLER_PORT` is set.

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use hyper_util::rt::TokioIo;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::{debug, error, info};

/// Configuration for the HTTP proxy.
struct ProxyConfig {
    /// The base URL of the user's web application (e.g. "http://127.0.0.1:8080").
    target_base: String,
    /// Optional base path to strip from incoming request paths.
    remove_base_path: Option<String>,
}

/// Run the HTTP reverse proxy server.
///
/// Listens on `listen_port` and forwards all requests to `target_base_url`.
/// If `remove_base_path` is set, strips that prefix from request paths before forwarding.
pub async fn run_http_proxy(
    listen_port: u16,
    target_base_url: String,
    remove_base_path: Option<String>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let addr = SocketAddr::from(([0, 0, 0, 0], listen_port));
    let listener = TcpListener::bind(addr).await?;

    info!(
        listen_port = listen_port,
        target = %target_base_url,
        remove_base_path = ?remove_base_path,
        "HTTP proxy listening"
    );

    let config = Arc::new(ProxyConfig {
        target_base: target_base_url,
        remove_base_path,
    });

    loop {
        let (stream, remote_addr) = listener.accept().await?;
        let io = TokioIo::new(stream);
        let config = config.clone();

        tokio::spawn(async move {
            let service = service_fn(move |req| {
                let config = config.clone();
                async move { proxy_request(req, &config).await }
            });

            if let Err(e) = http1::Builder::new().serve_connection(io, service).await {
                debug!(
                    remote_addr = %remote_addr,
                    error = %e,
                    "HTTP connection error"
                );
            }
        });
    }
}

/// Proxy a single HTTP request to the target application.
async fn proxy_request(
    req: Request<Incoming>,
    config: &ProxyConfig,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let path_and_query = uri
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");

    // Apply base path stripping if configured
    let stripped_path = if let Some(ref base) = config.remove_base_path {
        let (path, query) = path_and_query.split_once('?')
            .map(|(p, q)| (p, Some(q)))
            .unwrap_or((path_and_query, None));

        let mut new_path = if path.starts_with(base.as_str()) {
            path[base.len()..].to_string()
        } else {
            path.to_string()
        };

        if !new_path.starts_with('/') {
            new_path = format!("/{}", new_path);
        }

        match query {
            Some(q) => format!("{}?{}", new_path, q),
            None => new_path,
        }
    } else {
        path_and_query.to_string()
    };

    let target_url = format!("{}{}", config.target_base, stripped_path);

    debug!(
        method = %method,
        original_uri = %uri,
        target = %target_url,
        "proxying request"
    );

    // Build the outbound request
    let mut builder = Request::builder()
        .method(method.clone())
        .uri(&target_url);

    // Copy headers, skipping hop-by-hop headers
    for (key, value) in req.headers() {
        let name = key.as_str().to_lowercase();
        if matches!(
            name.as_str(),
            "host" | "transfer-encoding" | "connection" | "keep-alive"
        ) {
            continue;
        }
        builder = builder.header(key, value);
    }

    // Read the incoming body
    let body_bytes = match req.into_body().collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(e) => {
            error!(error = %e, "failed to read request body");
            return Ok(Response::builder()
                .status(502)
                .body(Full::new(Bytes::from("Bad Gateway: failed to read request body")))
                .unwrap());
        }
    };

    let outbound_req = match builder.body(Full::new(body_bytes)) {
        Ok(r) => r,
        Err(e) => {
            error!(error = %e, "failed to build proxy request");
            return Ok(Response::builder()
                .status(502)
                .body(Full::new(Bytes::from("Bad Gateway: failed to build request")))
                .unwrap());
        }
    };

    // Send to the target app
    let client: Client<_, Full<Bytes>> =
        Client::builder(TokioExecutor::new()).build_http();

    match client.request(outbound_req).await {
        Ok(response) => {
            let status = response.status();
            let headers = response.headers().clone();

            let body_bytes = match response.into_body().collect().await {
                Ok(collected) => collected.to_bytes(),
                Err(e) => {
                    error!(error = %e, "failed to read response body from app");
                    return Ok(Response::builder()
                        .status(502)
                        .body(Full::new(Bytes::from("Bad Gateway: failed to read response")))
                        .unwrap());
                }
            };

            let mut resp_builder = Response::builder().status(status);
            for (key, value) in &headers {
                resp_builder = resp_builder.header(key, value);
            }

            Ok(resp_builder.body(Full::new(body_bytes)).unwrap())
        }
        Err(e) => {
            error!(
                method = %method,
                target = %target_url,
                error = %e,
                "failed to proxy request to app"
            );
            Ok(Response::builder()
                .status(502)
                .body(Full::new(Bytes::from(format!(
                    "Bad Gateway: {}",
                    e
                ))))
                .unwrap())
        }
    }
}
