# Spring Boot on Azure Functions via Web Adapter

A standard Spring Boot application running on Azure Functions with zero Azure-specific code in the web app.

## How It Works

The Azure Functions Web Adapter sits between the Azure Functions Host and your Spring Boot app:

```
Azure Functions Host  --gRPC-->  Web Adapter  --HTTP-->  Spring Boot (:8080)
                      <--gRPC--               <--HTTP--
```

1. `func start` discovers the adapter via `workers/web-adapter/worker.config.json`
2. The host launches the adapter with gRPC worker arguments
3. The adapter starts the packaged Spring Boot JAR (`AZURE_FWA_STARTUP_COMMAND`)
4. The adapter polls `/api/health` on port `8080` until the app is ready
5. The adapter registers an HTTP catch-all function through worker-driven indexing
6. Each request is forwarded to Spring MVC as a normal HTTP request

## Project Structure

```
springboot/
├── host.json
├── local.settings.json
├── Dockerfile
├── workers/
│   └── web-adapter/
│       ├── azure-func-web-adapter
│       └── worker.config.json
└── app/
    ├── pom.xml
    └── src/
        └── main/
            ├── java/com/example/springboot/
            │   ├── SpringbootApplication.java
            │   └── ApiController.java
            └── resources/
                └── application.properties
```

## Configuration

| Variable | Value | Purpose |
|---|---|---|
| `FUNCTIONS_WORKER_RUNTIME` | `web-adapter` | Tells the host to use the web adapter worker |
| `AZURE_FWA_PORT` | `8080` | Port Spring Boot listens on |
| `AZURE_FWA_STARTUP_COMMAND` | `java -jar app/target/springboot-azure-func-web-adapter-0.0.1-SNAPSHOT.jar` | Starts the packaged Spring Boot app |
| `AZURE_FWA_READINESS_CHECK_PATH` | `/api/health` | Readiness endpoint for Spring Boot |

## Run Locally

```bash
# Build the Spring Boot jar
cd app && mvn clean package && cd ..

# Copy the built adapter binary into workers/
cp ../../target/release/azure-func-web-adapter workers/web-adapter/

# Start with Azure Functions Core Tools
func start
```

You can also run the Spring Boot app directly during development:

```bash
cd app
mvn spring-boot:run
```

Then browse `http://localhost:8080` to verify the app independently of Azure Functions.

## Endpoints

- `GET /` - welcome payload
- `GET /api/hello?name=World` - hello endpoint
- `POST /api/echo` - echoes the request body
- `GET /api/health` - readiness/health endpoint

## Deploy as a Container

```bash
docker build -t springboot-azure-func .
```

The Docker image performs a Maven build in a separate stage, then copies the packaged JAR into the Azure Functions base image with the web adapter worker.