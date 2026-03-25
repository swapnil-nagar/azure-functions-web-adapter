// Copyright (c) Azure Functions Web Adapter Contributors. All rights reserved.
// Licensed under the MIT License.

//! Configuration module for Azure Functions Web Adapter.
//!
//! All settings are read from environment variables using the `AZURE_FWA_` prefix.
//! This mirrors the Lambda Web Adapter's `AWS_LWA_` convention.

use std::env;
use std::time::Duration;

// ---------------------------------------------------------------------------
// Environment variable names
// ---------------------------------------------------------------------------

/// Port the user's web application listens on.
const ENV_PORT: &str = "AZURE_FWA_PORT";
/// Fallback port variable (matches common framework conventions).
const ENV_PORT_FALLBACK: &str = "PORT";
/// Host the user's web application binds to.
const ENV_HOST: &str = "AZURE_FWA_HOST";
/// Readiness check port (defaults to the traffic port).
const ENV_READINESS_CHECK_PORT: &str = "AZURE_FWA_READINESS_CHECK_PORT";
/// Readiness check HTTP path.
const ENV_READINESS_CHECK_PATH: &str = "AZURE_FWA_READINESS_CHECK_PATH";
/// Readiness check protocol: "http" or "tcp".
const ENV_READINESS_CHECK_PROTOCOL: &str = "AZURE_FWA_READINESS_CHECK_PROTOCOL";
/// Healthy HTTP status range (e.g. "200-399").
const ENV_READINESS_CHECK_HEALTHY_STATUS: &str = "AZURE_FWA_READINESS_CHECK_HEALTHY_STATUS";
/// Readiness check interval in milliseconds.
const ENV_READINESS_CHECK_INTERVAL_MS: &str = "AZURE_FWA_READINESS_CHECK_INTERVAL_MS";
/// Maximum time to wait for the app to become ready (seconds).
const ENV_READINESS_CHECK_TIMEOUT_S: &str = "AZURE_FWA_READINESS_CHECK_TIMEOUT_S";
/// Startup command for the web application (e.g. "node index.js").
const ENV_STARTUP_COMMAND: &str = "AZURE_FWA_STARTUP_COMMAND";
/// Base path to strip from incoming requests.
const ENV_REMOVE_BASE_PATH: &str = "AZURE_FWA_REMOVE_BASE_PATH";
/// Enable response compression.
const ENV_ENABLE_COMPRESSION: &str = "AZURE_FWA_ENABLE_COMPRESSION";

// ---------------------------------------------------------------------------
// Protocol enum
// ---------------------------------------------------------------------------

/// Protocol used for readiness health checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadinessProtocol {
    Http,
    Tcp,
}

impl ReadinessProtocol {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "tcp" => Self::Tcp,
            _ => Self::Http,
        }
    }
}

// ---------------------------------------------------------------------------
// Status range
// ---------------------------------------------------------------------------

/// A range of HTTP status codes considered "healthy".
#[derive(Debug, Clone)]
pub struct StatusRange {
    pub min: u16,
    pub max: u16,
}

impl StatusRange {
    /// Parse a range string like "200-399" or a single code like "200".
    pub fn parse(s: &str) -> Vec<Self> {
        s.split(',')
            .filter_map(|part| {
                let part = part.trim();
                if let Some((lo, hi)) = part.split_once('-') {
                    let lo = lo.trim().parse().ok()?;
                    let hi = hi.trim().parse().ok()?;
                    Some(StatusRange { min: lo, max: hi })
                } else {
                    let code = part.parse().ok()?;
                    Some(StatusRange {
                        min: code,
                        max: code,
                    })
                }
            })
            .collect()
    }

    /// Check if a status code falls within any of the ranges.
    pub fn contains(ranges: &[Self], code: u16) -> bool {
        ranges.iter().any(|r| code >= r.min && code <= r.max)
    }
}

// ---------------------------------------------------------------------------
// AdapterConfig
// ---------------------------------------------------------------------------

/// Complete configuration for the Azure Functions Web Adapter.
#[derive(Debug, Clone)]
pub struct AdapterConfig {
    /// Host the web app binds to.
    pub host: String,
    /// Port the web app listens on.
    pub port: u16,
    /// Port for readiness checks (defaults to `port`).
    pub readiness_check_port: u16,
    /// Path for readiness checks.
    pub readiness_check_path: String,
    /// Protocol for readiness checks.
    pub readiness_check_protocol: ReadinessProtocol,
    /// HTTP status codes considered healthy.
    pub readiness_healthy_status: Vec<StatusRange>,
    /// Interval between readiness check attempts.
    pub readiness_check_interval: Duration,
    /// Maximum wait time for the app to become ready.
    pub readiness_check_timeout: Duration,
    /// Command to start the user's web application.
    pub startup_command: Option<String>,
    /// Base path to remove from incoming requests.
    pub remove_base_path: Option<String>,
    /// Whether to compress responses.
    pub enable_compression: bool,
}

impl Default for AdapterConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 8080,
            readiness_check_port: 8080,
            readiness_check_path: "/".to_string(),
            readiness_check_protocol: ReadinessProtocol::Http,
            readiness_healthy_status: StatusRange::parse("100-499"),
            readiness_check_interval: Duration::from_millis(10),
            readiness_check_timeout: Duration::from_secs(120),
            startup_command: None,
            remove_base_path: None,
            enable_compression: false,
        }
    }
}

impl AdapterConfig {
    /// Build configuration from environment variables.
    pub fn from_env() -> Self {
        let port = env::var(ENV_PORT)
            .or_else(|_| env::var(ENV_PORT_FALLBACK))
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(8080u16);

        let readiness_check_port = env::var(ENV_READINESS_CHECK_PORT)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(port);

        let readiness_check_path = env::var(ENV_READINESS_CHECK_PATH)
            .unwrap_or_else(|_| "/".to_string());

        let readiness_check_protocol = env::var(ENV_READINESS_CHECK_PROTOCOL)
            .map(|v| ReadinessProtocol::from_str(&v))
            .unwrap_or(ReadinessProtocol::Http);

        let readiness_healthy_status = env::var(ENV_READINESS_CHECK_HEALTHY_STATUS)
            .map(|v| StatusRange::parse(&v))
            .unwrap_or_else(|_| StatusRange::parse("100-499"));

        let readiness_check_interval = env::var(ENV_READINESS_CHECK_INTERVAL_MS)
            .ok()
            .and_then(|v| v.parse().ok())
            .map(Duration::from_millis)
            .unwrap_or(Duration::from_millis(10));

        let readiness_check_timeout = env::var(ENV_READINESS_CHECK_TIMEOUT_S)
            .ok()
            .and_then(|v| v.parse().ok())
            .map(Duration::from_secs)
            .unwrap_or(Duration::from_secs(120));

        let host = env::var(ENV_HOST).unwrap_or_else(|_| "127.0.0.1".to_string());

        let startup_command = env::var(ENV_STARTUP_COMMAND).ok();

        let remove_base_path = env::var(ENV_REMOVE_BASE_PATH).ok();

        let enable_compression = env::var(ENV_ENABLE_COMPRESSION)
            .map(|v| v.to_lowercase() == "true" || v == "1")
            .unwrap_or(false);

        Self {
            host,
            port,
            readiness_check_port,
            readiness_check_path,
            readiness_check_protocol,
            readiness_healthy_status,
            readiness_check_interval,
            readiness_check_timeout,
            startup_command,
            remove_base_path,
            enable_compression,
        }
    }

    /// Base URL the user's web application is listening on.
    pub fn app_base_url(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }

    /// URL for readiness health checks.
    pub fn readiness_url(&self) -> String {
        format!(
            "http://{}:{}{}",
            self.host, self.readiness_check_port, self.readiness_check_path
        )
    }
}

// ---------------------------------------------------------------------------
// Worker startup args (parsed from CLI flags)
// ---------------------------------------------------------------------------

/// Startup arguments passed by the Azure Functions Host to the worker process.
#[derive(Debug, Clone)]
pub struct WorkerStartupArgs {
    /// gRPC address of the Azure Functions Host.
    pub functions_uri: String,
    /// Unique worker ID assigned by the host.
    pub worker_id: String,
    /// Request ID from the host.
    pub request_id: String,
    /// Maximum gRPC message size in bytes.
    pub grpc_max_message_length: usize,
}

impl WorkerStartupArgs {
    /// Parse startup arguments from command-line flags.
    ///
    /// Supports two formats:
    ///
    /// **Azure Functions Host format** (used by `func start`):
    /// ```text
    /// --host 127.0.0.1 --port 50051 --workerId <id> --requestId <id> --grpcMaxMessageLength <bytes>
    /// ```
    ///
    /// **Alternative format** (for direct invocation):
    /// ```text
    /// --functions-uri http://127.0.0.1:50051 --functions-worker-id <id>
    /// --functions-request-id <id> --functions-grpc-max-message-length <bytes>
    /// ```
    pub fn from_args() -> Result<Self, String> {
        let args: Vec<String> = env::args().collect();
        let mut functions_uri = String::new();
        let mut worker_id = String::new();
        let mut request_id = String::new();
        let mut grpc_max_message_length: usize = 128 * 1024 * 1024; // 128 MB default

        // For the host/port format used by func start
        let mut host = String::new();
        let mut port = String::new();

        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                // --- Azure Functions Host format (func start) ---
                "--host" => {
                    i += 1;
                    if i < args.len() {
                        host = args[i].clone();
                    }
                }
                "--port" => {
                    i += 1;
                    if i < args.len() {
                        port = args[i].clone();
                    }
                }
                "--workerId" => {
                    i += 1;
                    if i < args.len() {
                        worker_id = args[i].clone();
                    }
                }
                "--requestId" => {
                    i += 1;
                    if i < args.len() {
                        request_id = args[i].clone();
                    }
                }
                "--grpcMaxMessageLength" => {
                    i += 1;
                    if i < args.len() {
                        grpc_max_message_length =
                            args[i].parse().unwrap_or(128 * 1024 * 1024);
                    }
                }
                // --- Alternative format (direct invocation) ---
                "--functions-uri" => {
                    i += 1;
                    if i < args.len() {
                        functions_uri = args[i].clone();
                    }
                }
                "--functions-worker-id" => {
                    i += 1;
                    if i < args.len() {
                        worker_id = args[i].clone();
                    }
                }
                "--functions-request-id" => {
                    i += 1;
                    if i < args.len() {
                        request_id = args[i].clone();
                    }
                }
                "--functions-grpc-max-message-length" => {
                    i += 1;
                    if i < args.len() {
                        grpc_max_message_length =
                            args[i].parse().unwrap_or(128 * 1024 * 1024);
                    }
                }
                _ => {}
            }
            i += 1;
        }

        // If host/port format was used, construct the URI
        if functions_uri.is_empty() && !host.is_empty() && !port.is_empty() {
            functions_uri = format!("http://{}:{}", host, port);
        }

        if functions_uri.is_empty() || worker_id.is_empty() {
            return Err(
                "Missing required args: need --host/--port/--workerId (func start format) \
                 or --functions-uri/--functions-worker-id (direct format)"
                    .to_string(),
            );
        }

        Ok(Self {
            functions_uri,
            worker_id,
            request_id,
            grpc_max_message_length,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_range_parse_single() {
        let ranges = StatusRange::parse("200");
        assert_eq!(ranges.len(), 1);
        assert!(StatusRange::contains(&ranges, 200));
        assert!(!StatusRange::contains(&ranges, 201));
    }

    #[test]
    fn test_status_range_parse_range() {
        let ranges = StatusRange::parse("200-399");
        assert!(StatusRange::contains(&ranges, 200));
        assert!(StatusRange::contains(&ranges, 301));
        assert!(StatusRange::contains(&ranges, 399));
        assert!(!StatusRange::contains(&ranges, 400));
    }

    #[test]
    fn test_status_range_parse_multi() {
        let ranges = StatusRange::parse("200-299,404");
        assert!(StatusRange::contains(&ranges, 200));
        assert!(StatusRange::contains(&ranges, 250));
        assert!(StatusRange::contains(&ranges, 404));
        assert!(!StatusRange::contains(&ranges, 403));
    }

    #[test]
    fn test_default_config() {
        let config = AdapterConfig::default();
        assert_eq!(config.port, 8080);
        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.readiness_check_path, "/");
    }
}
