# Plan for db-performance-obvser

## Milestones
1) **Environment & Schema**
   - Add docker-compose with MySQL/PostgreSQL services and env overrides.
   - Add init SQL for `events` table and indexes; scripts to apply schema with/without secondary indexes.
2) **Rust CLI Skeleton**
   - Add CLI args and subcommands: `load` (data generation/loading) and `bench` (run scenarios).
   - Implement DB clients for MySQL/PostgreSQL with shared config parsing.
3) **Data Loader**
   - Implement generators for configured distributions and payload sizes.
   - Implement bulk loaders: Postgres COPY; MySQL LOAD DATA or batched INSERT fallback.
   - Add metrics (rows/s, errors) and row-count validation.
4) **Benchmark Scenarios**
   - Implement query cases: PK lookup, user_id lookup, created_at range, ORDER BY/LIMIT, update/delete optional.
   - Add warmup + timed runs, collect latency percentiles and throughput, emit CSV/JSON.
5) **Orchestration Scripts**
   - Add shell script to start/stop DB, load data, and run benchmarks across scales (1m→5000m).
   - Organize results under `results/{db}/{scale}/...`.
6) **Docs & Defaults**
   - Document configuration (env, flags, tuning presets) and usage examples.
   - Provide baseline compose settings for reproducible runs.

## Timeline/Order
- Start with Milestones 1–2 to get runnable skeleton.
- Build loader (3) before benchmarks (4).
- Add orchestration (5) once load + bench are usable.
- Finalize docs (6) after validation on at least one DB and one small scale.

## Open Decisions
- Exact DB versions for baseline (e.g., MySQL 8.0.x, Postgres 16.x).
- Default tuning knobs per engine (buffer/cache sizes).
- Whether to include wide-row payload profile in initial run or as optional flag.
