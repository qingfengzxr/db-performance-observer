# db-performance-obvser Design

## Goals
- Measure MySQL/PostgreSQL performance across table sizes: 1m/5m/10m/50m/100m/500m/1000m/2000m/5000m rows.
- Compare indexed vs non-indexed access for common OLTP-style queries.
- Produce reproducible, scriptable runs with captured metrics (throughput, latency p50/p95/p99) and CSV/JSON output.

## System Overview
Components:
1. **Container launcher**: `docker-compose.yml` (or `docker run` wrapper script) parameterized for MySQL/PostgreSQL version, ports, credentials, volumes. Includes init SQL to create schema and indexes.
2. **Schema/init SQL**: Creates `events` table and indexes; toggles optional indexes to compare runs.
3. **Data generator (Rust)**: Concurrently generates and bulk loads data for target scales; supports load modes (COPY/LOAD DATA vs batched INSERT). Configurable data distributions.
4. **Benchmark runner (Rust)**: Executes query suites with warmup and sampling, records metrics, and writes structured results. Can toggle target DB, scale, and index set.
5. **Orchestration script**: Shell script to cycle through scales, cold/hot runs, and output directories.

## Schema
```sql
CREATE TABLE events (
  id BIGINT PRIMARY KEY AUTO_INCREMENT, -- BIGSERIAL in Postgres
  user_id BIGINT NOT NULL,
  created_at TIMESTAMP NOT NULL,
  amount DECIMAL(10,2) NOT NULL,
  status SMALLINT NOT NULL,
  category INT NOT NULL,
  payload VARCHAR(200) NOT NULL,
  INDEX idx_user_created (user_id, created_at),
  INDEX idx_status (status)
);
```
Variants:
- Narrow rows: reduce `payload` to `VARCHAR(50)` to test row-size sensitivity.
- Wide rows: increase `payload` or switch to TEXT/JSONB.
- Index toggles: drop `idx_user_created` or `idx_status` to force table scans.

Data distributions (configurable):
- `user_id`: uniform or hotspot (e.g., Zipf) to test skew.
- `status`: low cardinality (0–4).
- `category`: medium cardinality (e.g., 0–5000).
- `created_at`: uniform over recent window.
- `amount`: bounded random.

## Data Generation
- Rust CLI subcommand `load`: takes target scale, db type, batch size, concurrency, distribution profile.
- Bulk load priority: PostgreSQL `COPY` via `tokio-postgres`/copy_in; MySQL `LOAD DATA LOCAL INFILE` or multi-row INSERT batches (5k–20k rows).
- Concurrency: split target rows into chunks per worker; generate rows in-memory and stream to DB; backpressure on connection errors.
- Telemetry: report rows/s and failures; optional checksum of row counts.

## Benchmarking
- Rust CLI subcommand `bench`: runs scenarios against populated DB:
  - Point lookup by PK (cache miss/hit).
  - Point lookup by indexed `user_id` with/without matching row.
  - Range query on `created_at` (small/medium/large windows).
  - ORDER BY created_at LIMIT/OFFSET (pagination).
  - Update by indexed filter; delete by range (optional).
- Each scenario: warmup N ops, then timed M ops; record latency distribution and throughput.
- Outputs: CSV/JSON per scenario with config metadata (scale, db type, indexes, seed, hardware notes).

## Orchestration
- `docker-compose.yml` exposes env vars for DB choice; optional `launch.sh` to start/stop containers and apply init SQL.
- `run_all.sh` iterates scales (1m→5000m), for each:
  1) create database, apply schema with/without indexes;
  2) load data; 3) run benchmarks (hot and optional cold run); 4) export results to `results/{db}/{scale}/...`.
- Supports rerun of specific scale via flags.

## Configuration
- CLI flags: `--db {mysql,postgres}`, `--scale`, `--batch-size`, `--concurrency`, `--distribution`, `--payload-size`, `--indexes on|off`.
- Compose env: port, user/pass, DB name, resource limits (memory), engine version.
- Tuning presets: MySQL (InnoDB buffer pool size, log file size), Postgres (shared_buffers, wal_level/checkpoints); keep a documented baseline.

## Metrics & Validation
- Collect: rows/s during load, total load time, errors; benchmark throughput and latency percentiles.
- Optional: sample `EXPLAIN ANALYZE` per scenario to confirm plan (index vs seq scan).
- Validation: count rows after load; verify indexes present/absent per run.

## Stretch Goals
- Add ClickHouse for columnar comparison.
- Add Grafana/Prometheus scrapes for resource usage.
- Support multiple payload profiles (JSON vs text) in one run.
