# Involute Agent Guide — lumen-pod

## 1. Project Binding

- **Repository**: `fakechris/lumen-pod`
- **Team Key**: `INV`
- **Root Project Identifier**: `INV-1135`
- **Root Project UUID**: `91579daf-73d2-4da3-be50-0d366d056e0b`

## 2. MCP Connection

| Agent Environment | MCP Endpoint URL |
|---|---|
| **Public / Remote** | `https://involute.lumenopen.com/mcp` (or `/mcp/readonly`) |
| **Local** | `http://127.0.0.1:4200/mcp` |

- **Auth Header**: `Authorization: Bearer <AGENT_TOKEN>`
- **Security Rule**: Never commit tokens.

## 3. Agent Rules

1. Search before propose. Duplicate titles are a failure.
2. Propose candidates; only humans commit.
3. Do not dump local plans/grep into Involute.
4. `work_claim` after user chooses — use `suggested_branch`.
5. Run complete → In Review. Never Done.
6. Unplanned fixes → `work_propose` with `DISCOVERED_DURING`.

## 4. Deployment

- **Host**: `oracle_5`（ssh config：`ubuntu@129.213.96.84`，ProxyCommand `nc -x localhost:7897`）
- **Deploy dir**: `/opt/pod`（git checkout，跟踪 `main`）
- **Production**: `https://pod.lumenopen.com`（Cloudflare + Caddy）
- **Stack**: `docker compose` 本地构建 — `pod-web` / `pod-api` / `pod-db`（postgres:16-alpine），数据卷 `pod_audiodata` / `pod_staticdata`
- **上线流程**: merge PR → 服务器 `cd /opt/pod && git pull && docker compose build && docker compose up -d` → 生产验证
- **Schema**: 迁移幂等（`schema.sql` 启动时在 pg_advisory_lock 下执行），无需手工 DDL
