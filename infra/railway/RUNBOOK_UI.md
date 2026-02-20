# Railway UI click-path runbook

This guide is the UI-version of deployment and troubleshooting for this project.
Follow it top-to-bottom inside Railway dashboard.

## 0) Prepare service names (important)

Use these service names exactly to match default configs:

- `platform`
- `postgres`
- `redis`
- `clickhouse`
- `nats`
- `loki`
- `prometheus`
- `grafana`

If you use different names, update references in:

- `infra/railway/prometheus/prometheus.yml`
- `infra/railway/grafana/provisioning/datasources/datasources.yml`
- platform env URLs

## 1) Create services (Root Directory)

In Railway project:

1. Click **New Service** -> **GitHub Repo**.
2. Pick this repository.
3. For each service below, set **Root Directory**:
  - `.` (platform)
   - `infra/railway/postgres`
   - `infra/railway/redis`
   - `infra/railway/clickhouse`
   - `infra/railway/nats`
   - `infra/railway/loki`
   - `infra/railway/prometheus`
   - `infra/railway/grafana`
4. In each service, open **Settings** and clear **Start Command** (leave empty).

## 2) Configure variables in UI

### 2.1 postgres

Open service `postgres` -> **Variables**:

- `POSTGRES_DB=platform`
- `POSTGRES_USER=postgres`
- `POSTGRES_PASSWORD=<strong-password>`

### 2.2 platform

Open service `platform` -> **Variables**:

- `DATABASE_URL=postgres://postgres:<POSTGRES_PASSWORD>@postgres:5432/platform`
- `REDIS_URL=redis://redis:6379`
- `NATS_URL=nats://nats:4222`
- `CLICKHOUSE_URL=http://clickhouse:8123`
- `JWT_SECRET=<strong-secret>`
- optional: `RUST_LOG=info`

### 2.3 grafana (optional)

Open service `grafana` -> **Variables**:

- `GF_SECURITY_ADMIN_USER=admin`
- `GF_SECURITY_ADMIN_PASSWORD=<strong-password>`

## 3) Attach volumes (stateful services)

For each service, open **Settings** -> **Volumes** -> **Add Volume**:

- `postgres`
- `redis`
- `clickhouse`
- optional: `grafana`

Without volumes, data may be lost after restart/redeploy.

## 4) Deploy in order

Deploy in this order:

1. `postgres`
2. `redis`
3. `clickhouse`
4. `nats`
5. `platform`
6. `loki`
7. `prometheus`
8. `grafana`

## 5) Verify health in UI

For each service, open **Deployments** and ensure latest deployment is Healthy.

Quick endpoint checks (service URL + path):

- platform: `/health`
- prometheus: `/-/healthy`
- grafana: `/api/health`

Expected result: HTTP 200 and healthy status.

## 6) If deployment fails (where to click)

1. Open failed service -> **Deployments** -> latest failed item.
2. Check **Build Logs** first:
   - If you see executable errors like `-js` or `-config.file...`, Start Command is wrong.
3. Check **Deploy Logs**:
   - Connection refused/timeouts usually indicate dependency not ready or wrong URL variable.
4. Open **Variables** tab and compare with section 2 above.
5. Redeploy after fix.

## 7) Fast fixes by symptom

- `The executable '-js' could not be found.`
  - Service: nats
  - Fix: clear Start Command, Root Directory `infra/railway/nats`, redeploy.

- `The executable '-config.file=...' could not be found.`
  - Service: loki
  - Fix: clear Start Command, Root Directory `infra/railway/loki`, redeploy.

- platform build log shows `cargo build --release` + `failed to find a workspace root`
  - Service: platform
  - Fix: set Root Directory to `.`, ensure Dockerfile builder is used, clear Start Command, redeploy.

- platform cannot connect to postgres/redis/nats/clickhouse
  - Fix: verify service names + platform env URLs, then redeploy dependencies first.

- prometheus has no platform metrics
  - Fix: ensure target in `infra/railway/prometheus/prometheus.yml` matches actual platform service name.

- grafana datasource errors
  - Fix: ensure datasource URLs match actual `prometheus` and `loki` service names.

## 8) Reference files

- `infra/railway/README.md`
- `infra/railway/RUNBOOK.md`
- `infra/railway/platform/railway.env.example`
- `infra/railway/postgres/railway.env.example`
- `infra/railway/redis/railway.env.example`
- `infra/railway/clickhouse/railway.env.example`
- `infra/railway/grafana/railway.env.example`
