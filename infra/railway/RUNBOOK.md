# Railway deployment runbook

## Service dependency map

- platform depends on: postgres, redis, nats, clickhouse
- prometheus depends on: platform (for /metrics)
- grafana depends on: prometheus, loki
- loki has no hard runtime dependency in this stack

## Recommended deployment order

1. postgres
2. redis
3. clickhouse
4. nats
5. platform
6. loki
7. prometheus
8. grafana

## Symptom -> likely cause -> check -> fix

| Symptom | Likely cause | What to check | Fix |
|---|---|---|---|
| platform fails at startup with DB connection errors | `DATABASE_URL` wrong or postgres not ready | platform logs for `sqlx`/connection refused; postgres service status | Set `DATABASE_URL=postgres://postgres:<POSTGRES_PASSWORD>@postgres:5432/platform`; redeploy postgres then platform |
| platform fails at startup with Redis errors | `REDIS_URL` wrong or redis unavailable | platform logs for redis pool/connect errors | Set `REDIS_URL=redis://redis:6379` (or auth URL if enabled) |
| platform fails at startup with NATS errors | `NATS_URL` wrong or nats unavailable | platform logs `async_nats` connect errors | Set `NATS_URL=nats://nats:4222`; verify nats service healthy |
| platform fails at startup with ClickHouse errors | `CLICKHOUSE_URL` wrong or clickhouse unavailable | platform logs around clickhouse client init | Set `CLICKHOUSE_URL=http://clickhouse:8123`; verify clickhouse service healthy |
| platform build fails with `couldn't read ... infra/sql/init.sql` | Docker build context excluded `infra/sql/init.sql` | Build Logs contain `include_str!(".../infra/sql/init.sql")` and `os error 2` | In repo `.dockerignore`, keep `infra/sql/init.sql` included (`!infra/sql/` + `!infra/sql/init.sql`) and redeploy |
| platform build fails with `Could not read "target/release/platform"` | `cargo-leptos` output binary name and bin target mismatch | Build Logs show server built as `platform-server` but `cargo-leptos` reads `target/release/platform` | In `apps/platform/Cargo.toml`, set `[package.metadata.leptos] output-name = "platform"` and `bin-target = "platform"`; provide `[[bin]] name = "platform"` target |
| platform build fails with `failed to find a workspace root` | platform built via Nixpacks in subdirectory | Build logs show `cargo build --release` and workspace inheritance errors | Set platform Root Directory to `.` and build with Dockerfile (`Dockerfile` + `railway.toml` at repo root) |
| nats build/deploy error: `The executable '-js' could not be found.` | Start command configured as args-only | Railway service Settings -> Start Command | Clear Start Command and use Root Directory `infra/railway/nats` |
| loki build/deploy error: `The executable '-config.file=...' could not be found.` | Start command configured as args-only | Railway service Settings -> Start Command | Clear Start Command and use Root Directory `infra/railway/loki` |
| prometheus is up but no platform metrics | scrape target name mismatch | prometheus targets page; service name in project | In `infra/railway/prometheus/prometheus.yml`, set target to `<platform-service-name>:3000` and redeploy |
| grafana datasource unreachable | prometheus/loki service names differ from defaults | Grafana datasource test, provisioning datasource URLs | Update datasource URLs to actual service names and redeploy grafana |
| data disappears after restart | no persistent volume attached | Railway volumes for postgres/redis/clickhouse | Attach persistent volumes to stateful services |

## Fast recovery checklist (10 minutes)

1. Confirm service names are exactly: `postgres`, `redis`, `clickhouse`, `nats`, `platform`, `loki`, `prometheus`, `grafana`.
2. Confirm platform Root Directory is `.` and other services use `infra/railway/*`.
3. Clear Start Command for all template-based services.
4. Re-apply platform env vars from `infra/railway/platform/railway.env.example`.
5. Redeploy in dependency order (in this document).
6. Verify `platform` health endpoint returns ok.
7. Verify prometheus target `platform:3000` is UP.
8. Open grafana and verify both datasources are healthy.
9. If startup logs show IMDS probe warnings in Railway, set `AWS_EC2_METADATA_DISABLED=true` for `platform`.

## Health endpoints

- platform: `/health`
- prometheus: `/-/healthy`
- grafana: `/api/health`

## Notes

- The app auto-binds Railway `PORT`; no manual port env is needed.
- For JWT, use either `JWT_SECRET` or RSA key pair vars.
