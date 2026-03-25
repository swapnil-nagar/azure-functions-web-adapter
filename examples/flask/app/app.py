"""
Flask application — runs on Azure Functions via Web Adapter.
This is a completely standard Flask app. No Azure SDK needed.
"""
import os
import signal
import sys
from datetime import datetime
from flask import Flask, jsonify, request

app = Flask(__name__)


@app.route("/")
def index():
    return jsonify(
        message="Hello from Flask on Azure Functions!",
        framework="Flask",
        adapter="Azure Functions Web Adapter",
        timestamp=datetime.utcnow().isoformat(),
    )


@app.route("/api/hello")
def hello():
    name = request.args.get("name", "World")
    return jsonify(message=f"Hello, {name}!")


@app.route("/api/echo", methods=["POST"])
def echo():
    return jsonify(
        received=request.get_json(silent=True),
        headers=dict(request.headers),
    )


@app.route("/api/health")
def health():
    return jsonify(status="healthy")


def handle_sigterm(*args):
    print("[flask] SIGTERM received, shutting down")
    sys.exit(0)


signal.signal(signal.SIGTERM, handle_sigterm)

if __name__ == "__main__":
    port = int(os.environ.get("PORT", 8080))
    app.run(host="0.0.0.0", port=port)
