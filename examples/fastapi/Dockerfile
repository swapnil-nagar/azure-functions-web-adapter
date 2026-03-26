FROM mcr.microsoft.com/azure-functions/dotnet:4

# Install Python
RUN apt-get update && apt-get install -y python3 python3-pip && \
    rm -rf /var/lib/apt/lists/*

# Copy Azure Functions configuration
COPY host.json /home/site/wwwroot/host.json

# Copy the web adapter worker
COPY workers/ /home/site/wwwroot/workers/
RUN chmod +x /home/site/wwwroot/workers/web-adapter/azure-func-web-adapter

# Copy FastAPI application
COPY app/ /home/site/wwwroot/app/
RUN pip3 install --no-cache-dir -r /home/site/wwwroot/app/requirements.txt

# Configuration (same pattern as AWS_LWA_* env vars)
ENV AzureWebJobsScriptRoot=/home/site/wwwroot \
    FUNCTIONS_WORKER_RUNTIME=web-adapter \
    AZURE_FWA_PORT=8080 \
    AZURE_FWA_STARTUP_COMMAND="uvicorn main:app --host 0.0.0.0 --port 8080 --app-dir /home/site/wwwroot/app" \
    AZURE_FWA_READINESS_CHECK_PATH="/" \
    AZURE_FWA_REMOVE_BASE_PATH="/api" \
    AZURE_FWA_LOG=info
