# jimeng-gateway

Video generation gateway for Jimeng/Seedance API.

## Features

- **Session Pool** — Multiple jimeng session IDs with LRU rotation, health checking, auto-disable
- **Async Task Queue** — Submit video generation tasks asynchronously, poll for progress
- **Queue Position Tracking** — Extract queue position & ETA from jimeng API responses
- **Docker Log Streaming** — Real-time container log viewing via API
- **React Dashboard** — Manage sessions, monitor tasks, view logs
- **Compatibility Layer** — Drop-in replacement for jimeng-free-api-all API format

## Architecture

```
StoryAI / Client
    │
    ▼
┌─────────────────────────────┐
│  jimeng-gateway (:5100)     │
│  ├─ REST API (axum)         │
│  ├─ Session Pool (LRU)      │
│  ├─ Task Queue (SQLite)     │
│  ├─ Docker Log Streamer     │
│  └─ React Dashboard         │
└─────────┬───────────────────┘
          │
          ▼
┌─────────────────────────────┐
│  jimeng-free-api-all (:8000)│
│  └─ Playwright (shark only) │
└─────────────────────────────┘
```

**Key insight**: Playwright is only needed for the initial submission (shark/a_bogus bypass, ~5 seconds). All polling and download operations use plain HTTP — the gateway handles these directly with no timeout limit.

## Quick Start

```bash
# Development
cp .env.example .env
cargo run

# Production (Docker)
docker compose up -d
```

## API

第三方接入 Seedance 2.0（精简版）请参考：`docs/seedance-2.0-integration.md`

### Tasks (async video generation)
```
POST   /api/v1/tasks              # Create task → immediate {task_id}
GET    /api/v1/tasks              # List tasks (?status=queued&limit=50)
GET    /api/v1/tasks/:id          # Task detail + queue position
POST   /api/v1/tasks/:id/cancel   # Cancel task
GET    /api/v1/stats              # Aggregate statistics
```

### Sessions (pool management)
```
GET    /api/v1/sessions           # List all sessions
POST   /api/v1/sessions           # Add {session_id, label?}
DELETE /api/v1/sessions/:id       # Remove
PATCH  /api/v1/sessions/:id       # Toggle {enabled: bool}
POST   /api/v1/sessions/:id/test  # Test validity
```

### Monitoring
```
GET    /api/v1/logs               # Container logs (?lines=100)
GET    /api/v1/health             # Health check + stats
```

### Compatibility (drop-in replacement)
```
POST   /v1/videos/generations     # Same format as jimeng API → async task
GET    /v1/models                 # Proxy to upstream
GET    /ping                      # Health check
```

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `PORT` | `5100` | Gateway listen port |
| `JIMENG_UPSTREAM` | `http://127.0.0.1:8000` | jimeng-free-api-all URL |
| `JIMENG_CONTAINER` | `jimeng-free-api-all` | Docker container name for logs |
| `DATABASE_URL` | `sqlite://data/gateway.db?mode=rwc` | SQLite database path |
| `CONCURRENCY` | `2` | Max concurrent video tasks |
| `MAX_POLL_DURATION_SECS` | `14400` | Max poll time (4 hours) |

## Tech Stack

- **Backend**: Rust (axum + tokio + sqlx/SQLite + bollard)
- **Frontend**: React 19 + Vite + TailwindCSS
- **Deployment**: Docker Compose (gateway + jimeng-free-api-all)
