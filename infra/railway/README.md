# Railway service templates (Platform / NATS / Loki / Prometheus / Grafana)

These templates lock the startup behavior into Dockerfiles so Railway won't fail due to mis-typed Start Command values.

## platform

- Service root: `infra/railway/platform`
- Runtime image: distroless with compiled `platform-server`
- Health check: `/health`
- Port binding: uses Railway `PORT` env automatically

Required environment variables:

- `DATABASE_URL`
- `REDIS_URL`
- `NATS_URL`
- `CLICKHOUSE_URL`
- `JWT_SECRET` (or `JWT_PRIVATE_KEY_PEM` + `JWT_PUBLIC_KEY_PEM`)

Quick copy template:

- `infra/railway/platform/railway.env.example`

## nats

- Service root: `infra/railway/nats`
- Runtime image: `nats:2.11`
- Effective start: `nats-server -js -m 8222`

## loki

- Service root: `infra/railway/loki`
- Runtime image: `grafana/loki:3.3.2`
- Effective start: `/usr/bin/loki -config.file=/etc/loki/local-config.yaml`

## prometheus

- Service root: `infra/railway/prometheus`
- Runtime image: `prom/prometheus:v3.2.1`
- Effective start: image default (`prometheus`) with copied config
- Default scrape target: `platform:3000` (change if your platform service name differs)

Optional template notes:

- `infra/railway/prometheus/railway.env.example`

## grafana

- Service root: `infra/railway/grafana`
- Runtime image: `grafana/grafana:11.5.2`
- Effective start: image default (`grafana-server`) with copied provisioning

Optional variables template:

- `infra/railway/grafana/railway.env.example`

## How to use in Railway

1. Create a service from this repo.
2. Set **Root Directory** to one of:
   - `infra/railway/platform`
   - `infra/railway/nats`
   - `infra/railway/loki`
   - `infra/railway/prometheus`
   - `infra/railway/grafana`
3. Leave Start Command empty (Dockerfile CMD is used).
4. Redeploy.

## Why this fixes your error

Your previous logs show Railway trying to execute argument-only strings (`-js`, `-config.file=...`) as binaries.
With these Dockerfiles, the base image entrypoint receives the args correctly.
