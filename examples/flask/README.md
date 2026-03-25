# Flask on Azure Functions via Web Adapter

A standard Flask application running on Azure Functions **with zero code changes**.

## Project Structure

```
flask/
├── host.json
├── worker.config.json
├── Dockerfile
└── app/
    ├── app.py                # Standard Flask app (NO CHANGES)
    └── requirements.txt
```

## Configuration

| Variable | Value | Purpose |
|---|---|---|
| `AZURE_FWA_PORT` | `8080` | Port Flask/Gunicorn listens on |
| `AZURE_FWA_STARTUP_COMMAND` | `gunicorn --bind 0.0.0.0:8080 app:app` | Start with Gunicorn |
| `AZURE_FWA_READINESS_CHECK_PATH` | `/` | Health check path |

## Run Locally

```bash
cd app
pip install -r requirements.txt
python app.py
# → http://localhost:8080
```
