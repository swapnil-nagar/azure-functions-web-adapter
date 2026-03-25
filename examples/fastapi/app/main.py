"""
FastAPI application — runs on Azure Functions via Web Adapter.
This is a completely standard FastAPI app. No Azure SDK needed.
"""
import os
from datetime import datetime
from fastapi import FastAPI, Query
from pydantic import BaseModel

app = FastAPI(
    title="FastAPI on Azure Functions",
    description="Running via Azure Functions Web Adapter",
    version="1.0.0",
)


class EchoRequest(BaseModel):
    message: str
    data: dict | None = None


class EchoResponse(BaseModel):
    received: EchoRequest
    timestamp: str


@app.get("/")
async def index():
    return {
        "message": "Hello from FastAPI on Azure Functions!",
        "framework": "FastAPI",
        "adapter": "Azure Functions Web Adapter",
        "timestamp": datetime.utcnow().isoformat(),
    }


@app.get("/api/hello")
async def hello(name: str = Query(default="World")):
    return {"message": f"Hello, {name}!"}


@app.post("/api/echo", response_model=EchoResponse)
async def echo(body: EchoRequest):
    return EchoResponse(
        received=body,
        timestamp=datetime.utcnow().isoformat(),
    )


@app.get("/api/health")
async def health():
    return {"status": "healthy"}


@app.get("/api/items/{item_id}")
async def get_item(item_id: int, q: str | None = None):
    result = {"item_id": item_id}
    if q:
        result["query"] = q
    return result
