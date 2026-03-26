# Express.js on Azure Functions via Web Adapter

A standard Express.js application running on Azure Functions **with zero code changes** — just like [AWS Lambda Web Adapter](https://github.com/awslabs/aws-lambda-web-adapter).

## How It Works

The Azure Functions Web Adapter sits between the Azure Functions Host and your Express.js app:

```
Azure Functions Host  ──gRPC──>  Web Adapter  ──HTTP──>  Express.js (:8080)
                      <──gRPC──               <──HTTP──
```

1. `func start` discovers the adapter via `workers/web-adapter/worker.config.json`
2. Host launches the adapter with gRPC connection arguments
3. Adapter spawns Express.js (`AZURE_FWA_STARTUP_COMMAND`)
4. Adapter polls `http://localhost:8080/` until Express is ready
5. Adapter registers an HTTP catch-all function via gRPC (no `function.json` needed!)
6. On each request: Host → gRPC → Adapter → HTTP → Express → HTTP → Adapter → gRPC → Host

## Project Structure

```
expressjs/
├── host.json                       # Standard Azure Functions host config (no customHandler!)
├── local.settings.json             # AZURE_FWA_* env vars + FUNCTIONS_WORKER_RUNTIME
├── Dockerfile                      # Container deployment
├── workers/
│   └── web-adapter/
│       ├── azure-func-web-adapter  # The adapter binary
│       └── worker.config.json      # Worker discovery config
└── app/
    ├── index.js                    # Standard Express.js app (ZERO CHANGES)
    └── package.json
```

**Compare with AWS Lambda Web Adapter:**

| | AWS Lambda Web Adapter | Azure Functions Web Adapter |
|---|---|---|
| Adapter delivery | Lambda Layer or Docker COPY | `workers/` directory or Docker COPY |
| Config | `AWS_LWA_*` env vars | `AZURE_FWA_*` env vars |
| App changes needed | None | None |
| Function definitions | Not needed | Not needed (worker-driven indexing) |

## The Express.js App (Zero Changes!)

```javascript
const express = require('express');
const app = express();
const port = process.env.PORT || 8080;

app.get('/', (req, res) => {
    res.json({ message: 'Hello from Express.js on Azure Functions!' });
});

app.listen(port, () => {
    console.log(`Express.js app listening at http://localhost:${port}`);
});
```

This is a **completely standard** Express.js server. No Azure SDK, no special handler, no function.json.

## Configuration

| Variable | Value | Purpose |
|---|---|---|
| `FUNCTIONS_WORKER_RUNTIME` | `web-adapter` | Tells the host to use our worker |
| `AZURE_FWA_PORT` | `8080` | Port Express listens on |
| `AZURE_FWA_STARTUP_COMMAND` | `node app/index.js` | Command to start Express |
| `AZURE_FWA_READINESS_CHECK_PATH` | `/` | Health check endpoint |
| `AZURE_FWA_REMOVE_BASE_PATH` | `/api` | Strip `/api` prefix from routes |

## Run Locally

```bash
# Install dependencies
cd app && npm install && cd ..

# Copy the built adapter binary into workers/
cp ../../target/release/azure-func-web-adapter workers/web-adapter/

# Start with Azure Functions Core Tools
func start
```

## Deploy to Azure

### Container deployment
```bash
# Build the Docker image
docker build -t expressjs-azure-func .

# Push to Azure Container Registry
az acr build --registry <registry> --image expressjs-azure-func .

# Deploy to Azure Functions
az functionapp create \
    --name <app-name> \
    --resource-group <rg> \
    --storage-account <storage> \
    --image <registry>.azurecr.io/expressjs-azure-func:latest \
    --functions-version 4
```
