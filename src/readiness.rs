// Copyright (c) Azure Functions Web Adapter Contributors. All rights reserved.
// Licensed under the MIT License.

//! Readiness check module.
//!
//! Polls the user's web application until it responds with a healthy status code,
//! or a TCP connection succeeds. This is directly inspired by the Lambda Web
//! Adapter's readiness check mechanism.

use crate::config::{AdapterConfig, ReadinessProtocol, StatusRange};
use std::time::Instant;
use tokio::net::TcpStream;
use tracing::{debug, info};

/// Error returned when readiness check times out.
#[derive(Debug)]
pub struct ReadinessTimeoutError {
    pub elapsed: std::time::Duration,
}

impl std::fmt::Display for ReadinessTimeoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "web application did not become ready within {:?}",
            self.elapsed
        )
    }
}

impl std::error::Error for ReadinessTimeoutError {}

/// Poll the web application until it becomes ready.
///
/// Returns `Ok(elapsed_duration)` on success or `Err(ReadinessTimeoutError)` on timeout.
pub async fn wait_until_ready(config: &AdapterConfig) -> Result<std::time::Duration, ReadinessTimeoutError> {
    let start = Instant::now();
    let deadline = start + config.readiness_check_timeout;

    info!(
        protocol = ?config.readiness_check_protocol,
        port = config.readiness_check_port,
        path = %config.readiness_check_path,
        "starting readiness check"
    );

    loop {
        if Instant::now() >= deadline {
            return Err(ReadinessTimeoutError {
                elapsed: start.elapsed(),
            });
        }

        let ready = match config.readiness_check_protocol {
            ReadinessProtocol::Http => check_http(config).await,
            ReadinessProtocol::Tcp => check_tcp(config).await,
        };

        if ready {
            let elapsed = start.elapsed();
            info!(?elapsed, "web application is ready");
            return Ok(elapsed);
        }

        tokio::time::sleep(config.readiness_check_interval).await;
    }
}

/// Perform an HTTP health check. Returns true if the response status is in
/// the healthy range.
async fn check_http(config: &AdapterConfig) -> bool {
    let url = config.readiness_url();
    debug!(url = %url, "HTTP readiness check");

    // Use a simple TCP+HTTP/1.1 request to avoid pulling in a heavy HTTP client
    // just for health checks.
    let addr = format!("{}:{}", config.host, config.readiness_check_port);
    let stream = match TcpStream::connect(&addr).await {
        Ok(s) => s,
        Err(_) => return false,
    };

    // Build a minimal HTTP/1.1 GET request
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}:{}\r\nConnection: close\r\n\r\n",
        config.readiness_check_path, config.host, config.readiness_check_port,
    );

    // Write request
    if let Err(_) = stream.try_write(request.as_bytes()) {
        return false;
    }

    // Read response (we only need the status line)
    let mut buf = [0u8; 256];
    stream.readable().await.ok();
    match stream.try_read(&mut buf) {
        Ok(n) if n > 0 => {
            let response = String::from_utf8_lossy(&buf[..n]);
            // Parse status code from "HTTP/1.1 200 OK"
            if let Some(status_str) = response.split_whitespace().nth(1) {
                if let Ok(code) = status_str.parse::<u16>() {
                    let healthy = StatusRange::contains(&config.readiness_healthy_status, code);
                    debug!(status = code, healthy, "HTTP readiness response");
                    return healthy;
                }
            }
            false
        }
        _ => false,
    }
}

/// Perform a TCP connection check. Returns true if a TCP connection succeeds.
async fn check_tcp(config: &AdapterConfig) -> bool {
    let addr = format!("{}:{}", config.host, config.readiness_check_port);
    debug!(addr = %addr, "TCP readiness check");
    TcpStream::connect(&addr).await.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_tcp_check_fails_on_unused_port() {
        let config = AdapterConfig {
            host: "127.0.0.1".to_string(),
            readiness_check_port: 59999, // unlikely to be in use
            ..AdapterConfig::default()
        };
        assert!(!check_tcp(&config).await);
    }
}
