# Azure Functions Web Adapter

Run **any** web application on Azure Functions — without code changes.

Azure Functions Web Adapter allows developers to build web apps with familiar frameworks (Express.js, Flask, FastAPI, Spring Boot, ASP.NET, Nginx, etc.) and run them on Azure Functions. The same container image can run on Azure Functions, Azure Container Apps, AKS, and local machines.

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│  Azure Functions Container                              │
│                                                         │
│  ┌──────────────────┐       ┌────────────────────────┐  │
│  │ Azure Functions  │ gRPC  │  Web Adapter           │  │
│  │ Host             │◀─────▶│  (Rust binary)         │  │
│  └──────────────────┘       │                        │  │
│                             │  Translates gRPC       │  │
│                             │  InvocationRequest     │  │
│                             │  ↔ HTTP request        │  │
│                             └───────────┬────────────┘  │
│                                         │ HTTP          │
│                             ┌───────────▼────────────┐  │
│                             │  Your Web App          │  │
│                             │  (Express/Flask/etc)   │  │
│                             │  localhost:8080        │  │
│                             └────────────────────────┘  │
└─────────────────────────────────────────────────────────┘
```

## How It Works

1. **Azure Functions Host** starts the Web Adapter as a language worker (via `worker.config.json`)
2. **Web Adapter** connects to the host over gRPC (`FunctionRpc.EventStream`)
3. Web Adapter **spawns your web app** as a child process
4. Web Adapter **polls** `http://localhost:8080/` until your app is ready
5. Web Adapter **registers** an HTTP catch-all function with the host
6. On each request:
   - Host sends `InvocationRequest` (gRPC) → Adapter converts to HTTP request
   - Adapter forwards to your app on `localhost:8080`
   - Your app responds with standard HTTP → Adapter converts to `InvocationResponse` (gRPC)
   - Host returns the response to the client

**Your web app never knows it's running on Azure Functions.**

## Quick Start

### 1. Add the adapter to your project

```
my-function-app/
├── host.json
├── worker.config.json         ← Points to the adapter binary
├── azure-func-web-adapter     ← The adapter binary
└── app/
    ├── index.js               ← Your standard Express.js app
    └── package.json
```

### 2. Configure `worker.config.json`

```json
{
    "description": {
        "language": "web-adapter",
        "defaultExecutablePath": "{AzureWebJobsScriptRoot}/azure-func-web-adapter",
        "workerIndexing": "true"
    }
}
```

### 3. Set environment variables

```bash
AZURE_FWA_PORT=8080                              # Port your app listens on
AZURE_FWA_STARTUP_COMMAND="node app/index.js"    # Command to start your app
AZURE_FWA_READINESS_CHECK_PATH="/"               # Health check endpoint
```

### 4. Deploy

```bash
func azure functionapp publish <app-name>
```

## Features

- **Zero code changes** — run any HTTP web framework on Azure Functions
- **Any language/framework** — Express.js, Flask, FastAPI, Spring Boot, ASP.NET, Nginx, Go, Rust, etc.
- **gRPC native** — deep integration with Azure Functions Host via `FunctionRpc` protocol
- **Readiness checks** — HTTP or TCP health checks before accepting traffic
- **Placeholder/proxy mode** — supports consumption plan with fixed base images
- **Graceful shutdown** — SIGTERM propagation to child processes
- **Binary response handling** — auto-detects and handles binary content
- **Configurable** — all settings via environment variables

## Configuration

| Variable | Description | Default |
|---|---|---|
| `AZURE_FWA_PORT` | Port your app listens on (falls back to `PORT`) | `8080` |
| `AZURE_FWA_HOST` | Host your app binds to | `127.0.0.1` |
| `AZURE_FWA_STARTUP_COMMAND` | Command to start your web app | None |
| `AZURE_FWA_READINESS_CHECK_PORT` | Readiness check port | Same as PORT |
| `AZURE_FWA_READINESS_CHECK_PATH` | Readiness check path | `/` |
| `AZURE_FWA_READINESS_CHECK_PROTOCOL` | `http` or `tcp` | `http` |
| `AZURE_FWA_READINESS_CHECK_HEALTHY_STATUS` | Healthy HTTP status range | `100-499` |
| `AZURE_FWA_READINESS_CHECK_INTERVAL_MS` | Check interval (ms) | `10` |
| `AZURE_FWA_READINESS_CHECK_TIMEOUT_S` | Max wait time (seconds) | `120` |
| `AZURE_FWA_REMOVE_BASE_PATH` | Base path to strip from requests | None |
| `AZURE_FWA_ENABLE_COMPRESSION` | Enable response compression | `false` |
| `AZURE_FWA_MODE` | `proxy` for placeholder mode | (direct) |
| `AZURE_FWA_LOG` | Log level (`trace`/`debug`/`info`/`warn`/`error`) | `info` |

## Modes

### Direct Mode (Default)

The adapter connects directly to the Azure Functions Host, spawns your web app, and handles all requests. Used for **dedicated plans** and **local development**.

```
Host  ──gRPC──>  Web Adapter  ──HTTP──>  Your App
```

### Proxy/Placeholder Mode

For **consumption plans** with fixed base images. A lightweight proxy handles the pre-warm phase. On specialization, it spawns the full adapter pipeline.

```
Phase 1 (placeholder):
Host  ──gRPC──>  Proxy (stub responses, container warm)

Phase 2 (specialization):
Host  ──gRPC──>  Proxy  ──channel──>  Adapter  ──HTTP──>  Your App
```

Set `AZURE_FWA_MODE=proxy` and `WEBSITE_PLACEHOLDER_MODE=1` for this mode.

## Examples

| Framework | Language | Example |
|---|---|---|
| Express.js | Node.js | [examples/expressjs](examples/expressjs) |
| Flask | Python | [examples/flask](examples/flask) |
| FastAPI | Python | [examples/fastapi](examples/fastapi) |

## Design Principles

This project combines the best ideas from two proven architectures:

| Concept | Source | How We Use It |
|---|---|---|
| HTTP ↔ Event translation | [AWS Lambda Web Adapter](https://github.com/awslabs/aws-lambda-web-adapter) | Convert gRPC `InvocationRequest` to HTTP, and HTTP responses to `InvocationResponse` |
| gRPC worker protocol | [Azure Functions Go Worker](https://github.com/Azure/azure-functions-golang-worker) | Implement `FunctionRpc.EventStream` for native host integration |
| Proxy/placeholder model | [Azure Functions Go Worker](https://github.com/Azure/azure-functions-golang-worker) | Support fixed base images via proxy that spawns child on specialization |
| Readiness polling | [AWS Lambda Web Adapter](https://github.com/awslabs/aws-lambda-web-adapter) | HTTP/TCP health checks before accepting traffic |
| worker.config.json | Azure Functions convention | Register the adapter as a language worker |
| Environment variables | Both | All config via env vars (`AZURE_FWA_*` prefix) |

## Building

```bash
# Build for your platform
cargo build --release

# Cross-compile for Linux (Azure Functions default)
cargo build --release --target x86_64-unknown-linux-gnu

# The binary is at target/release/azure-func-web-adapter
```

## Technology Choice: Rust

The adapter is written in **Rust** for maximum performance:

- **~2ms** overhead per request (gRPC decode → HTTP forward → gRPC encode)
- **~5MB** static binary with no runtime dependencies
- **Zero garbage collection** pauses
- **Memory safe** without runtime overhead
- Same language as the [AWS Lambda Web Adapter](https://github.com/awslabs/aws-lambda-web-adapter), proving this approach at scale

## License

MIT
