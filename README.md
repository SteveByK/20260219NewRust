# 社交地图 + 实时通讯 + 高性能网页游戏

## 已完成能力

- 登录/注册：JWT 鉴权
- 社交地图：MapLibre 实时点位刷新
- 实时通讯：WebSocket + MessagePack
- 聊天：发送消息 + 历史消息加载
- 聊天状态：房间成员在线状态 + 未读计数 + 已读标记
- 邀请：在线用户发起对战邀请 + 接受/拒绝状态流转

## 前端入口

- 主页联调面板：`apps/platform/src/app.rs`
- 地图控制与点位图层：`apps/platform/src/map.rs`

## 核心 API

- `POST /api/register`
- `POST /api/login`
- `POST /api/position`
- `POST /api/chat/send`
- `GET /api/chat/history?room_id=global`
- `GET /api/chat/room-state?token=...&room_id=global`
- `POST /api/chat/mark-read`
- `POST /api/invite/send`
- `GET /api/invite/pending?token=...`
- `POST /api/invite/respond`
- `GET /ws?token=...`

## 启动

```powershell
docker compose up -d
cargo leptos watch
```

## 一键联调脚本

已提供端到端联调脚本：

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\e2e-social-flow.ps1
```

该脚本会自动验证：

- 登录/注册
- 聊天发送
- 未读计数变化 + 标记已读
- 邀请发送 + 待处理列表 + 接受邀请

## CI 集成（已接入）

`GitHub Actions` 已新增 `e2e-social-flow` 任务，流程为：

1. 启动 `postgres / redis / nats / clickhouse`
2. 后台启动 `platform-server`
3. 执行 `scripts/e2e-social-flow.ps1`

工作流文件：`.github/workflows/ci.yml`

## Railway 报错原因与修复

你图里的两个错误：

- nats: `The executable '-js' could not be found.`
- loki: `The executable '-config.file=/etc/loki/local-config.yaml' could not be found.`

根因一致：Railway 当前把你填写的启动命令当成“可执行文件”本体来运行了。
也就是你传了参数（`-js` / `-config.file=...`），但没有传主程序名，所以容器启动失败。

### 修复方式（Railway 服务 Start Command）

- NATS 服务：

```text
nats-server -js -m 8222
```

- Loki 服务：

```text
/usr/bin/loki -config.file=/etc/loki/local-config.yaml
```

### 额外建议

- 这些监控组件（`loki/prometheus/grafana`）建议先不放 Railway 生产主环境，可放独立监控环境。
- 如果仍报路径错误，先进入容器确认二进制路径：`which nats-server`、`which loki`。

### 配置固化（避免 UI 手改丢失）

已提供 Railway 模板目录，可直接在 Railway 设置对应 `Root Directory` 部署：

- `.`（platform 服务使用仓库根目录 + 根 Dockerfile）
- `infra/railway/postgres`
- `infra/railway/redis`
- `infra/railway/clickhouse`
- `infra/railway/nats`
- `infra/railway/loki`
- `infra/railway/prometheus`
- `infra/railway/grafana`

详细说明见：`infra/railway/README.md`

变量模板示例：

- `infra/railway/platform/railway.env.example`
- `infra/railway/postgres/railway.env.example`
- `infra/railway/redis/railway.env.example`
- `infra/railway/clickhouse/railway.env.example`
- `infra/railway/grafana/railway.env.example`
- `infra/railway/prometheus/railway.env.example`

## Railway 一键核对清单（复制即用）

> 按顺序执行，避免 90% 部署失败。

1. 服务命名统一：`platform / postgres / redis / clickhouse / nats / loki / prometheus / grafana`
2. Root Directory 正确：
	- `platform` 用 `.`
	- 其他服务用 `infra/railway/<service>`
3. 所有服务清空 Start Command（使用 Dockerfile 默认 CMD）
4. platform 变量完整：
	- `DATABASE_URL=postgres://postgres:<POSTGRES_PASSWORD>@postgres:5432/platform`
	- `REDIS_URL=redis://redis:6379`
	- `NATS_URL=nats://nats:4222`
	- `CLICKHOUSE_URL=http://clickhouse:8123`
	- `JWT_SECRET=<strong-secret>`
	- 可选：`RUST_LOG=info`
	- 可选：`AWS_EC2_METADATA_DISABLED=true`
5. 部署顺序：
	- `postgres -> redis -> clickhouse -> nats -> platform -> loki -> prometheus -> grafana`
6. 健康检查：
	- platform: `/health`
	- prometheus: `/-/healthy`
	- grafana: `/api/health`
7. 若 platform Build Logs 出现 `couldn't read ... infra/sql/init.sql`：检查仓库 `.dockerignore` 必须放行 `infra/sql/init.sql`
8. 若 platform Build Logs 出现 `Could not read "target/release/platform"`：检查 `apps/platform/Cargo.toml` 的 `[package.metadata.leptos]` 中 `output-name`/`bin-target` 与 `[[bin]] name = "platform"` 一致

完整排障说明：

- `infra/railway/RUNBOOK.md`
- `infra/railway/RUNBOOK_UI.md`

## Day7（上线前预检 + 回滚预案）

上线前建议执行：

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\deploy-preflight.ps1 -BaseUrl https://<platform-url>
```

回滚预演（不改仓库）：

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\deploy-rollback.ps1 -KnownGoodCommit <commit>
```

执行回滚（生成 revert commit）：

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\deploy-rollback.ps1 -KnownGoodCommit <commit> -Apply
```

可选：在 GitHub Actions 手动触发 `ci` 工作流并填写 `preflight_base_url`，会运行 `preflight-remote` 作业对目标环境执行 `/health`、`/ready`、`/` 预检。
该输入要求为远程 `https://` 地址，且不允许 `localhost/127.0.0.1/::1`。
