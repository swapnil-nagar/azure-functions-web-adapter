# Next.js on Azure Functions via Web Adapter

A standard Next.js application running on Azure Functions **with zero code changes** — just like [AWS Lambda Web Adapter](https://github.com/awslabs/aws-lambda-web-adapter).

## How It Works

The Azure Functions Web Adapter sits between the Azure Functions Host and your Next.js app:

```
Azure Functions Host  ──gRPC──>  Web Adapter  ──HTTP──>  Next.js standalone (:8080)
                      <──gRPC──               <──HTTP──
```

1. `func start` discovers the adapter via `workers/web-adapter/worker.config.json`
2. Host launches the adapter with gRPC connection arguments
3. Adapter spawns Next.js standalone server (`AZURE_FWA_STARTUP_COMMAND`)
4. Adapter polls `http://localhost:8080/` until Next.js is ready
5. Adapter registers an HTTP catch-all function via gRPC (no `function.json` needed!)
6. On each request: Host → gRPC → Adapter → HTTP → Next.js → HTTP → Adapter → gRPC → Host

## Project Structure

```
nextjs/
├── host.json                       # Standard Azure Functions host config
├── local.settings.json             # AZURE_FWA_* env vars + FUNCTIONS_WORKER_RUNTIME
├── Dockerfile                      # Container deployment
├── workers/
│   └── web-adapter/
│       ├── azure-func-web-adapter  # The adapter binary (build artifact, .gitignored)
│       └── worker.config.json      # Worker discovery config
└── app/
    ├── index.js                    # Wrapper to launch standalone server on port 8080
    ├── next.config.ts              # output: "standalone"
    ├── package.json
    └── app/
        ├── page.tsx                # Home page (SSR)
        └── api/
            ├── route.ts            # GET /api
            ├── hello/route.ts      # GET /api/hello
            ├── echo/route.ts       # POST /api/echo
            └── health/route.ts     # GET /api/health
```

## Configuration

| Variable | Value | Purpose |
|---|---|---|
| `FUNCTIONS_WORKER_RUNTIME` | `web-adapter` | Tells the host to use our worker |
| `AZURE_FWA_PORT` | `8080` | Port Next.js listens on |
| `AZURE_FWA_STARTUP_COMMAND` | `node app/index.js` | Command to start Next.js standalone |
| `AZURE_FWA_READINESS_CHECK_PATH` | `/` | Health check endpoint |

## Run Locally

```bash
# Install dependencies and build Next.js in standalone mode
cd app && npm install && npm run build && cd ..

# Copy static files to standalone output (required by Next.js)
cp -r app/.next/static app/.next/standalone/.next/static
cp -r app/public app/.next/standalone/public

# Copy the built adapter binary into workers/
cp ../../target/release/azure-func-web-adapter workers/web-adapter/

# Start with Azure Functions Core Tools
func start
```

## API Routes

- `GET /` — Next.js home page (SSR)
- `GET /api` — JSON info endpoint
- `GET /api/hello?name=World` — Hello endpoint
- `POST /api/echo` — Echo request body and headers
- `GET /api/health` — Health check

## Key Notes

- **Standalone output**: `next.config.ts` sets `output: "standalone"` to produce a self-contained Node.js server
- **Wrapper script**: `app/index.js` sets `PORT=8080` and `HOSTNAME=0.0.0.0` before requiring the standalone server
- **Static files**: Must be copied to `.next/standalone/.next/static` and `.next/standalone/public` after building
- **No code changes**: The Next.js app is completely standard — no Azure SDK, no special handlers
