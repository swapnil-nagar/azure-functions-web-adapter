FROM mcr.microsoft.com/azure-functions/dotnet:4

# Copy the web adapter
COPY ./azure-func-web-adapter /home/site/wwwroot/azure-func-web-adapter
RUN chmod +x /home/site/wwwroot/azure-func-web-adapter

# Install Python
RUN apt-get update && apt-get install -y python3 python3-pip && \
    rm -rf /var/lib/apt/lists/*

# Copy configuration
COPY worker.config.json /home/site/wwwroot/worker.config.json
COPY host.json /home/site/wwwroot/host.json

# Copy FastAPI application
COPY app/ /home/site/wwwroot/app/
RUN pip3 install --no-cache-dir -r /home/site/wwwroot/app/requirements.txt

# Environment configuration
ENV AzureWebJobsScriptRoot=/home/site/wwwroot \
    AzureFunctionsJobHost__Logging__Console__IsEnabled=true \
    AZURE_FWA_PORT=8080 \
    AZURE_FWA_STARTUP_COMMAND="uvicorn main:app --host 0.0.0.0 --port 8080 --app-dir /home/site/wwwroot/app" \
    AZURE_FWA_READINESS_CHECK_PATH="/" \
    AZURE_FWA_LOG=info

EXPOSE 8080
