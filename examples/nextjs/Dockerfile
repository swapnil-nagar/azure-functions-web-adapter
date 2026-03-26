# -------------------------------------------------------------------
# Next.js on Azure Functions via Web Adapter
# -------------------------------------------------------------------
# This Dockerfile packages a standard Next.js app (standalone mode)
# to run on Azure Functions — exactly like AWS Lambda Web Adapter.
#
# Your Next.js app needs ZERO code changes. The web adapter handles
# all Azure Functions protocol translation transparently.
# -------------------------------------------------------------------

FROM mcr.microsoft.com/azure-functions/dotnet:4

# Install Node.js
RUN apt-get update && apt-get install -y curl && \
    curl -fsSL https://deb.nodesource.com/setup_18.x | bash - && \
    apt-get install -y nodejs && \
    rm -rf /var/lib/apt/lists/*

# Copy Azure Functions configuration
COPY host.json /home/site/wwwroot/host.json

# Copy the web adapter worker
COPY workers/ /home/site/wwwroot/workers/
RUN chmod +x /home/site/wwwroot/workers/web-adapter/azure-func-web-adapter

# Copy the Next.js standalone build output
COPY app/.next/standalone/ /home/site/wwwroot/app/.next/standalone/
COPY app/.next/static/ /home/site/wwwroot/app/.next/standalone/.next/static/
COPY app/public/ /home/site/wwwroot/app/.next/standalone/public/

# Copy the wrapper script
COPY app/index.js /home/site/wwwroot/app/index.js

# Configuration
ENV AzureWebJobsScriptRoot=/home/site/wwwroot \
    FUNCTIONS_WORKER_RUNTIME=web-adapter \
    AZURE_FWA_PORT=8080 \
    AZURE_FWA_STARTUP_COMMAND="node /home/site/wwwroot/app/index.js" \
    AZURE_FWA_READINESS_CHECK_PATH="/" \
    AZURE_FWA_LOG=info
