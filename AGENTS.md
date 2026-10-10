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
