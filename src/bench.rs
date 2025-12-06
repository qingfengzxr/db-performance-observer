use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, Result};
use rand::rngs::StdRng;
use rand::Rng;
use rand::SeedableRng;
use serde::Serialize;
use tokio::task::JoinSet;
use tokio::time::Instant;
use tokio_postgres::Client as PgClient;
use mysql_async::prelude::Queryable;

use crate::config::{DbConfig, DbKind};
use crate::load::fetch_mysql_max_id;
use crate::load::fetch_postgres_max_id;

pub struct BenchConfig {
    pub warmup_ops: u64,
    pub sample_ops: u64,
    pub concurrency: usize,
    pub output: Option<PathBuf>,
    pub seed: u64,
}

#[derive(Debug, Clone, Copy)]
enum ParamKind {
    None,
    PkHit,
    UserHit,
}

#[derive(Debug, Clone)]
struct Scenario {
    name: &'static str,
    mysql_sql: &'static str,
    postgres_sql: &'static str,
    param: ParamKind,
}

#[derive(Serialize)]
struct BenchResult {
    scenario: String,
    ops: u64,
    throughput_ops: f64,
    avg_ms: f64,
    p50_ms: f64,
    p95_ms: f64,
    p99_ms: f64,
}

#[derive(Debug)]
struct Stats {
    avg: f64,
    p50: f64,
    p95: f64,
    p99: f64,
}

pub async fn run_bench(db: DbConfig, cfg: BenchConfig) -> Result<()> {
    let results = match db.kind {
        DbKind::Mysql => bench_mysql(&db.url, &cfg).await?,
        DbKind::Postgres => bench_postgres(&db.url, &cfg).await?,
    };

    let json = serde_json::to_string_pretty(&results)?;
    println!("{}", json);

    if let Some(path) = &cfg.output {
        tokio::fs::write(path, json).await?;
        tracing::info!("基准结果已写入 {:?}", path);
    }

    Ok(())
}

async fn bench_mysql(url: &str, cfg: &BenchConfig) -> Result<Vec<BenchResult>> {
    let pool = mysql_async::Pool::new(mysql_async::Opts::from_url(url)?);
    let max_id = fetch_mysql_max_id(&pool).await?;
    if max_id == 0 {
        return Err(anyhow!("events 表为空，无法基准测试"));
    }

    let scenarios = scenarios();
    let mut results = Vec::with_capacity(scenarios.len());
    for sc in scenarios {
        let res = run_mysql_scenario(&pool, &sc, cfg, max_id).await?;
        results.push(res);
    }
    pool.disconnect().await?;
    Ok(results)
}

async fn bench_postgres(url: &str, cfg: &BenchConfig) -> Result<Vec<BenchResult>> {
    let (client, connection) = tokio_postgres::connect(url, tokio_postgres::NoTls).await?;
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            tracing::error!("Postgres 连接任务出错: {}", e);
        }
    });
    let max_id = fetch_postgres_max_id(&client).await?;
    if max_id == 0 {
        return Err(anyhow!("events 表为空，无法基准测试"));
    }

    let scenarios = scenarios();
    let mut results = Vec::with_capacity(scenarios.len());
    for sc in scenarios {
        let res = run_postgres_scenario(url, &sc, cfg, max_id).await?;
        results.push(res);
    }
    Ok(results)
}

fn scenarios() -> Vec<Scenario> {
    vec![
        Scenario {
            name: "pk_hit",
            mysql_sql: "SELECT id FROM events WHERE id = ?",
            postgres_sql: "SELECT id FROM events WHERE id = $1",
            param: ParamKind::PkHit,
        },
        Scenario {
            name: "user_lookup",
            mysql_sql: "SELECT id FROM events WHERE user_id = ? ORDER BY created_at DESC LIMIT 1",
            postgres_sql: "SELECT id FROM events WHERE user_id = $1 ORDER BY created_at DESC LIMIT 1",
            param: ParamKind::UserHit,
        },
        Scenario {
            name: "range_small",
            mysql_sql: "SELECT id FROM events WHERE created_at BETWEEN DATE_SUB(NOW(), INTERVAL 1 DAY) AND NOW() ORDER BY created_at DESC LIMIT 50",
            postgres_sql: "SELECT id FROM events WHERE created_at BETWEEN (NOW() - INTERVAL '1 day') AND NOW() ORDER BY created_at DESC LIMIT 50",
            param: ParamKind::None,
        },
        Scenario {
            name: "range_large",
            mysql_sql: "SELECT id FROM events WHERE created_at BETWEEN DATE_SUB(NOW(), INTERVAL 30 DAY) AND NOW() ORDER BY created_at DESC LIMIT 200",
            postgres_sql: "SELECT id FROM events WHERE created_at BETWEEN (NOW() - INTERVAL '30 day') AND NOW() ORDER BY created_at DESC LIMIT 200",
            param: ParamKind::None,
        },
        Scenario {
            name: "order_page",
            mysql_sql: "SELECT id FROM events ORDER BY created_at DESC LIMIT 50 OFFSET 100",
            postgres_sql: "SELECT id FROM events ORDER BY created_at DESC LIMIT 50 OFFSET 100",
            param: ParamKind::None,
        },
    ]
}

async fn run_mysql_scenario(
    pool: &mysql_async::Pool,
    sc: &Scenario,
    cfg: &BenchConfig,
    max_id: u64,
) -> Result<BenchResult> {
    let workers = cfg.concurrency.max(1) as u64;
    let warm_base = cfg.warmup_ops / workers;
    let warm_rem = cfg.warmup_ops % workers;
    let sample_base = cfg.sample_ops / workers;
    let sample_rem = cfg.sample_ops % workers;

    let mut tasks = JoinSet::new();
    let mut durations: Vec<f64> = Vec::with_capacity(cfg.sample_ops as usize);
    let durations_shared = Arc::new(tokio::sync::Mutex::new(Vec::with_capacity(
        cfg.sample_ops as usize,
    )));
    let progress = Arc::new(AtomicU64::new(0));

    let scenario_start = Instant::now();
    for worker_id in 0..workers {
        let warm = warm_base + if worker_id < warm_rem { 1 } else { 0 };
        let sample = sample_base + if worker_id < sample_rem { 1 } else { 0 };
        let pool = pool.clone();
        let sc = sc.clone();
        let max_id = max_id;
        let durations_shared = durations_shared.clone();
        let progress = progress.clone();
        let seed = cfg.seed;
        tasks.spawn(async move {
            let mut conn = pool.get_conn().await?;
            let mut rng = StdRng::seed_from_u64(seed + worker_id);
            // warmup
            for _ in 0..warm {
                exec_mysql(&mut conn, &sc, &mut rng, max_id).await?;
            }

            for _ in 0..sample {
                let start = Instant::now();
                exec_mysql(&mut conn, &sc, &mut rng, max_id).await?;
                let elapsed = start.elapsed().as_secs_f64() * 1000.0;
                let mut guard = durations_shared.lock().await;
                guard.push(elapsed);
                let done = progress.fetch_add(1, Ordering::Relaxed) + 1;
                if done % 500 == 0 {
                    let rps = done as f64 / scenario_start.elapsed().as_secs_f64().max(0.001);
                    tracing::info!("scenario={} mysql 已完成 {} 次采样, {:.2} ops/s", sc.name, done, rps);
                }
            }

            conn.disconnect().await?;
            Ok::<(), anyhow::Error>(())
        });
    }

    while let Some(res) = tasks.join_next().await {
        res??;
    }

    let mut guard = durations_shared.lock().await;
    durations.append(&mut guard);
    let stats = calc_stats(&mut durations);
    let wall = scenario_start.elapsed().as_secs_f64();
    let throughput = cfg.sample_ops as f64 / wall.max(0.001);

    Ok(BenchResult {
        scenario: sc.name.to_string(),
        ops: cfg.sample_ops,
        throughput_ops: throughput,
        avg_ms: stats.avg,
        p50_ms: stats.p50,
        p95_ms: stats.p95,
        p99_ms: stats.p99,
    })
}

async fn run_postgres_scenario(
    url: &str,
    sc: &Scenario,
    cfg: &BenchConfig,
    max_id: u64,
) -> Result<BenchResult> {
    let workers = cfg.concurrency.max(1) as u64;
    let warm_base = cfg.warmup_ops / workers;
    let warm_rem = cfg.warmup_ops % workers;
    let sample_base = cfg.sample_ops / workers;
    let sample_rem = cfg.sample_ops % workers;

    let mut tasks = JoinSet::new();
    let mut durations: Vec<f64> = Vec::with_capacity(cfg.sample_ops as usize);
    let durations_shared = Arc::new(tokio::sync::Mutex::new(Vec::with_capacity(
        cfg.sample_ops as usize,
    )));
    let progress = Arc::new(AtomicU64::new(0));
    let scenario_start = Instant::now();

    for worker_id in 0..workers {
        let warm = warm_base + if worker_id < warm_rem { 1 } else { 0 };
        let sample = sample_base + if worker_id < sample_rem { 1 } else { 0 };
        let url = url.to_string();
        let sc = sc.clone();
        let durations_shared = durations_shared.clone();
        let progress = progress.clone();
        let seed = cfg.seed;
        tasks.spawn(async move {
            let (client, connection) = tokio_postgres::connect(&url, tokio_postgres::NoTls).await?;
            tokio::spawn(async move {
                if let Err(e) = connection.await {
                    tracing::error!("Postgres worker 连接任务出错: {}", e);
                }
            });
            let mut rng = StdRng::seed_from_u64(seed + worker_id);

            for _ in 0..warm {
                exec_postgres(&client, &sc, &mut rng, max_id).await?;
            }

            for _ in 0..sample {
                let start = Instant::now();
                exec_postgres(&client, &sc, &mut rng, max_id).await?;
                let elapsed = start.elapsed().as_secs_f64() * 1000.0;
                let mut guard = durations_shared.lock().await;
                guard.push(elapsed);
                let done = progress.fetch_add(1, Ordering::Relaxed) + 1;
                if done % 500 == 0 {
                    let rps = done as f64 / scenario_start.elapsed().as_secs_f64().max(0.001);
                    tracing::info!("scenario={} postgres 已完成 {} 次采样, {:.2} ops/s", sc.name, done, rps);
                }
            }

            Ok::<(), anyhow::Error>(())
        });
    }

    while let Some(res) = tasks.join_next().await {
        res??;
    }

    let mut guard = durations_shared.lock().await;
    durations.append(&mut guard);
    let stats = calc_stats(&mut durations);
    let wall = scenario_start.elapsed().as_secs_f64();
    let throughput = cfg.sample_ops as f64 / wall.max(0.001);

    Ok(BenchResult {
        scenario: sc.name.to_string(),
        ops: cfg.sample_ops,
        throughput_ops: throughput,
        avg_ms: stats.avg,
        p50_ms: stats.p50,
        p95_ms: stats.p95,
        p99_ms: stats.p99,
    })
}

async fn exec_mysql(
    conn: &mut mysql_async::Conn,
    sc: &Scenario,
    rng: &mut StdRng,
    max_id: u64,
) -> Result<()> {
    match sc.param {
        ParamKind::None => {
            let _: Option<(i64,)> = conn.exec_first(sc.mysql_sql, ()).await?;
        }
        ParamKind::PkHit => {
            let id = rng.gen_range(1..=max_id as i64);
            let _: Option<(i64,)> = conn.exec_first(sc.mysql_sql, (id,)).await?;
        }
        ParamKind::UserHit => {
            let user_id = rng.gen_range(1..=1_000_000_i64);
            let _: Option<(i64,)> = conn.exec_first(sc.mysql_sql, (user_id,)).await?;
        }
    }
    Ok(())
}

async fn exec_postgres(
    client: &PgClient,
    sc: &Scenario,
    rng: &mut StdRng,
    max_id: u64,
) -> Result<()> {
    match sc.param {
        ParamKind::None => {
            let _ = client.query_opt(sc.postgres_sql, &[]).await?;
        }
        ParamKind::PkHit => {
            let id = rng.gen_range(1..=max_id as i64);
            let _ = client.query_opt(sc.postgres_sql, &[&id]).await?;
        }
        ParamKind::UserHit => {
            let user_id = rng.gen_range(1..=1_000_000_i64);
            let _ = client.query_opt(sc.postgres_sql, &[&user_id]).await?;
        }
    }
    Ok(())
}

fn calc_stats(durations_ms: &mut Vec<f64>) -> Stats {
    if durations_ms.is_empty() {
        return Stats {
            avg: 0.0,
            p50: 0.0,
            p95: 0.0,
            p99: 0.0,
        };
    }
    durations_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let sum: f64 = durations_ms.iter().sum();
    let avg = sum / durations_ms.len() as f64;
    let idx = |p: f64| -> usize {
        let pos = (p * durations_ms.len() as f64).ceil() as usize;
        durations_ms.len().saturating_sub(1).min(pos.saturating_sub(1))
    };
    Stats {
        avg,
        p50: durations_ms[idx(0.50)],
        p95: durations_ms[idx(0.95)],
        p99: durations_ms[idx(0.99)],
    }
}
