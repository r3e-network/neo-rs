---
kind: external_dependency
name: Prometheus metrics exporter
slug: prometheus
category: external_dependency
category_hints:
    - vendor_identity
scope:
    - '**'
---

### Prometheus
- Role: Telemetry/metrics exporter exposing a `/metrics` endpoint alongside the health endpoints (`/healthz`, `/readyz`).
- Durable usage model: Enabled together with the health server; scrape target configured via docker-compose monitoring profile or external Prometheus setup.