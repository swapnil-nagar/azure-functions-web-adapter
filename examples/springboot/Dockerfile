# -------------------------------------------------------------------
# Spring Boot on Azure Functions via Web Adapter
# -------------------------------------------------------------------
# This Dockerfile packages a standard Spring Boot app to run on
# Azure Functions through azure-functions-web-adapter.
#
# The Spring Boot application stays framework-native. The adapter
# translates Azure Functions gRPC invocations to HTTP requests.
# -------------------------------------------------------------------

FROM maven:3.9.9-eclipse-temurin-17 AS build

WORKDIR /workspace/app
COPY app/pom.xml ./pom.xml
COPY app/src ./src
RUN mvn -B clean package -DskipTests

FROM mcr.microsoft.com/azure-functions/dotnet:4

# Install Java runtime for the Spring Boot application.
RUN apt-get update && apt-get install -y openjdk-17-jre-headless && \
    rm -rf /var/lib/apt/lists/*

# Copy Azure Functions configuration
COPY host.json /home/site/wwwroot/host.json

# Copy the web adapter worker
COPY workers/ /home/site/wwwroot/workers/
RUN chmod +x /home/site/wwwroot/workers/web-adapter/azure-func-web-adapter

# Copy the packaged Spring Boot application
COPY --from=build /workspace/app/target/springboot-azure-func-web-adapter-0.0.1-SNAPSHOT.jar /home/site/wwwroot/app/target/springboot-azure-func-web-adapter-0.0.1-SNAPSHOT.jar

# Configuration
ENV AzureWebJobsScriptRoot=/home/site/wwwroot \
    FUNCTIONS_WORKER_RUNTIME=web-adapter \
    AZURE_FWA_PORT=8080 \
    AZURE_FWA_STARTUP_COMMAND="java -jar /home/site/wwwroot/app/target/springboot-azure-func-web-adapter-0.0.1-SNAPSHOT.jar" \
    AZURE_FWA_READINESS_CHECK_PATH="/api/health" \
    AZURE_FWA_LOG=info