# Express.js on Azure Functions via Web Adapter

A standard Express.js application running on Azure Functions **with zero code changes**.

## How It Works

The Azure Functions Web Adapter sits between the Azure Functions Host and your Express.js app:

```
Azure Functions Host  ──gRPC──>  Web Adapter  ──HTTP──>  Express.js (:8080)
                      <──gRPC──               <──HTTP──
```

1. Azure Functions Host starts the Web Adapter as a worker (via `worker.config.json`)
2. Web Adapter spawns your Express.js app (`node index.js`)
3. Web Adapter polls `http://localhost:8080/` until Express is ready
4. Web Adapter registers an HTTP catch-all function with the host
5. On each request: Host → gRPC InvocationRequest → Adapter → HTTP → Express → HTTP Response → Adapter → gRPC InvocationResponse → Host

## Project Structure

```
expressjs/
├── host.json                 # Azure Functions host config
├── worker.config.json        # Points to the web adapter binary
├── local.settings.json       # Local development settings
├── Dockerfile                # Container deployment
└── app/
    ├── index.js              # Standard Express.js app (NO CHANGES)
    └── package.json
```

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

This is a **completely standard** Express.js server. No Azure SDK, no special handler.

## Configuration

| Variable | Value | Purpose |
|---|---|---|
| `AZURE_FWA_PORT` | `8080` | Port Express listens on |
| `AZURE_FWA_STARTUP_COMMAND` | `node app/index.js` | Command to start Express |
| `AZURE_FWA_READINESS_CHECK_PATH` | `/` | Health check endpoint |

## Run Locally

```bash
# Install dependencies
cd app && npm install && cd ..

# Start Express.js directly (for local testing)
node app/index.js

# Or with Azure Functions Core Tools
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

### Zip deployment (with adapter layer)
```bash
# Build adapter, copy to project
cp ../../target/release/azure-func-web-adapter .

# Deploy
func azure functionapp publish <app-name>
```
