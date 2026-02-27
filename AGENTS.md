# jimeng-gateway — Agent Instructions

## Project Overview

Video generation gateway for Jimeng/Seedance API. Rust backend (axum) + React frontend, deployed natively on jpdata server via systemd.

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Backend | Rust 2024 edition, axum 0.8, tokio, sqlx (SQLite) |
| Frontend | React 19, Vite, TailwindCSS |
| Auth | OIDC SSO (Rauthy) + API key auth |
| Runtime | systemd service, native binary on jpdata |
| Deployment | GitHub Actions CI → SSH → native build on server |
| Dependencies | jimeng-free-api-all (Docker), rauthy (Docker) |

## Project Structure

```
src/
├── main.rs              # Entry point, server setup
├── config.rs            # Environment config parsing
├── auth/                # OIDC SSO + API key middleware
├── db/                  # SQLite schema, migrations, queries
├── docker/              # Bollard Docker API integration (log streaming)
├── pool/                # Session pool (LRU rotation, health check)
├── queue/               # Async task queue (submit, poll, cancel)
└── routes/              # Axum route handlers
web/
├── src/                 # React dashboard
├── vite.config.js
└── tailwind.config.js
scripts/
├── deploy.sh            # Server-side deploy script (git pull → build → restart)
└── jimeng-gateway.service  # systemd unit file
docker-compose.yml         # Only for jimeng-api + rauthy (NOT gateway)
```

## Conventions

### Rust
- Use `anyhow::Result` for application errors, `thiserror` for library-style typed errors
- All async functions use tokio runtime
- Config via `dotenvy` + environment variables (see `.env.example`)
- Database queries use sqlx with compile-time checked SQL where possible
- Logging: `tracing` crate with `RUST_LOG` env filter

### Frontend
- React functional components with hooks
- TailwindCSS for styling, no CSS modules
- Vite dev server proxies `/api` to backend

## Deployment

Gateway runs as a **native binary** managed by systemd. No Docker for gateway itself.

- **Target**: jpdata (185.200.65.233, Ubuntu 22.04, 4 cores, 8GB RAM)
- **Runtime**: systemd service (`jimeng-gateway.service`)
- **CI**: GitHub Actions → SSH → `scripts/deploy.sh` (git pull → cargo build --release → npm build → systemctl restart)
- **Secrets**: `JPDATA_SSH_KEY` (GitHub repo secret, already configured)
- **Server path**: `/opt/jimeng-gateway`
- **Rust**: 1.93.1 via rustup (`~/.cargo/env`)
- **Node.js**: v22 (system install)
- **Logs**: `journalctl -u jimeng-gateway -f`

### Manual deploy
```bash
ssh jpdata "bash /opt/jimeng-gateway/scripts/deploy.sh"
```

### Service management
```bash
ssh jpdata "systemctl status jimeng-gateway"
ssh jpdata "journalctl -u jimeng-gateway -n 100 -f"
ssh jpdata "systemctl restart jimeng-gateway"
```

## Key Environment Variables

| Variable | Description |
|----------|-------------|
| `PORT` | Gateway listen port (default: 5100) |
| `JIMENG_UPSTREAM` | jimeng-free-api URL |
| `JIMENG_CONTAINER` | Docker container name for log streaming |
| `DATABASE_URL` | SQLite connection string |
| `CONCURRENCY` | Max concurrent video generation tasks |
| `AUTH_ENABLED` | Enable API key authentication |
| `OIDC_ISSUER_URL` | OIDC provider URL for SSO |

## Important Notes

- **Gateway 不使用 Docker**，以 systemd 原生服务运行
- jimeng-free-api-all 和 rauthy 仍以 Docker 容器运行（`docker-compose.yml` 只管这两个）
- SQLite DB 文件在 `/opt/jimeng-gateway/data/`，systemd 服务有 ReadWritePaths 权限
- `.env` 文件在服务器上手动管理，不进 git
- 服务器还运行 `frps` 容器（与本项目无关，勿动）
- Dockerfile 已移除，不再需要
