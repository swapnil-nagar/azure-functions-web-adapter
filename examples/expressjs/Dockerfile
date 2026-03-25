# -------------------------------------------------------------------
# Express.js on Azure Functions via Web Adapter
# -------------------------------------------------------------------
# This Dockerfile packages a standard Express.js app to run on
# Azure Functions. The ONLY addition is copying the web adapter binary.
# -------------------------------------------------------------------

FROM mcr.microsoft.com/azure-functions/dotnet:4

# Copy the pre-built web adapter binary
# In production, this would come from an official release
COPY ./azure-func-web-adapter /home/site/wwwroot/azure-func-web-adapter
RUN chmod +x /home/site/wwwroot/azure-func-web-adapter

# Install Node.js
RUN apt-get update && apt-get install -y curl && \
    curl -fsSL https://deb.nodesource.com/setup_18.x | bash - && \
    apt-get install -y nodejs && \
    rm -rf /var/lib/apt/lists/*

# Copy worker configuration
COPY worker.config.json /home/site/wwwroot/worker.config.json
COPY host.json /home/site/wwwroot/host.json

# Copy Express.js application
COPY app/ /home/site/wwwroot/app/
WORKDIR /home/site/wwwroot/app
RUN npm install --omit=dev

# Environment configuration
ENV AzureWebJobsScriptRoot=/home/site/wwwroot \
    AzureFunctionsJobHost__Logging__Console__IsEnabled=true \
    AZURE_FWA_PORT=8080 \
    AZURE_FWA_STARTUP_COMMAND="node /home/site/wwwroot/app/index.js" \
    AZURE_FWA_READINESS_CHECK_PATH="/" \
    AZURE_FWA_LOG=info

EXPOSE 8080
