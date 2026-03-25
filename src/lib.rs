// Copyright (c) Azure Functions Web Adapter Contributors. All rights reserved.
// Licensed under the MIT License.

//! # Azure Functions Web Adapter
//!
//! Run **any** web application on Azure Functions without code changes.
//!
//! This adapter acts as a bridge between the Azure Functions Host (gRPC) and
//! your standard HTTP web application (Express.js, Flask, FastAPI, Spring Boot,
//! ASP.NET, Nginx, etc.).
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │  Azure Functions Container                              │
//! │                                                         │
//! │  ┌──────────────────┐       ┌────────────────────────┐  │
//! │  │ Azure Functions  │       │                        │  │
//! │  │ Host             │ gRPC  │  Web Adapter           │  │
//! │  │                  │◀─────▶│                        │  │
//! │  └──────────────────┘       │  Translates gRPC       │  │
//! │                             │  InvocationRequest     │  │
//! │                             │  ↔ HTTP request        │  │
//! │                             │         │              │  │
//! │                             └─────────┼──────────────┘  │
//! │                                       │ HTTP            │
//! │                             ┌─────────▼──────────────┐  │
//! │                             │  Your Web App          │  │
//! │                             │  (Express/Flask/etc)   │  │
//! │                             │  localhost:8080        │  │
//! │                             └────────────────────────┘  │
//! └─────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Modes
//!
//! - **Direct mode**: Adapter connects directly to the host, spawns your web
//!   app, and handles all requests. Used in dedicated plans and local dev.
//!
//! - **Proxy/Placeholder mode**: A lightweight proxy handles the pre-warm phase.
//!   On specialization, it spawns the full adapter + your web app. Used in
//!   consumption plans with fixed base images.

pub mod config;
pub mod grpc_handler;
pub mod http_forwarder;
pub mod http_proxy;
pub mod process;
pub mod proxy;
pub mod readiness;

/// Re-export generated protobuf types.
pub mod proto {
    tonic::include_proto!("azure_functions_worker");
}

/// Crate-level error type.
pub type Error = Box<dyn std::error::Error + Send + Sync>;
