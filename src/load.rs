use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::{Datelike, Timelike};
use mysql_async::{prelude::*, Conn as MyConn, Params as MyParams, Pool as MyPool, Value as MyValue};
use tokio::task::JoinSet;
use tokio::time::Instant;
use tokio_postgres::types::ToSql;
use tokio_postgres::Client as PgClient;

use crate::config::{DbConfig, Distribution, IndexMode};
use crate::generator::{EventGenerator, EventRow};

pub struct LoadConfig {
    pub scale: u64,
    pub concurrency: usize,
    pub batch_size: usize,
    pub distribution: Distribution,
    pub payload_size: usize,
    pub indexes: IndexMode,
}

pub async fn run_load(db: DbConfig, cfg: LoadConfig) -> Result<()> {
    // 先读取当前行数，按需补齐到目标规模
    let current = match db.kind {
        crate::config::DbKind::Mysql => count_mysql_rows(&db.url).await?,
        crate::config::DbKind::Postgres => count_postgres_rows(&db.url).await?,
    };
    if current >= cfg.scale {
        tracing::info!(
            "当前已有 {} 行，已达到/超过目标 {}，跳过装载",
            current,
            cfg.scale
        );
        return Ok(());
    }

    let remaining = cfg.scale - current;
    tracing::info!(
        "当前已有 {} 行，目标 {} 行，本次需新增 {} 行",
        current,
        cfg.scale,
        remaining
    );

    let mut generator = EventGenerator::new(cfg.distribution, cfg.payload_size);

    match db.kind {
        crate::config::DbKind::Mysql => {
            load_mysql(&db.url, &cfg, remaining, &mut generator).await?
        }
        crate::config::DbKind::Postgres => {
            load_postgres(&db.url, &cfg, remaining, &mut generator).await?
        }
    }

    Ok(())
}

async fn load_mysql(url: &str, cfg: &LoadConfig, remaining: u64, _gen: &mut EventGenerator) -> Result<()> {
    let pool = MyPool::new(mysql_async::Opts::from_url(url)?);
    {
        let mut conn = pool
            .get_conn()
            .await
            .with_context(|| format!("连接 MySQL 失败: {}", url))?;
        configure_mysql_indexes(&mut conn, cfg.indexes).await?;
        conn.disconnect().await?;
    }

    let workers = cfg.concurrency.max(1).min(remaining as usize);
    let base_quota = remaining / workers as u64;
    let remainder = remaining % workers as u64;
    let total = Arc::new(AtomicU64::new(0));
    let start = Instant::now();

    let mut tasks = JoinSet::new();
    for worker_id in 0..workers {
        let quota = base_quota + if worker_id < remainder as usize { 1 } else { 0 };
        let mut generator =
            EventGenerator::with_seed(cfg.distribution, cfg.payload_size, worker_id as u64 + 1);
        let pool = pool.clone();
        let batch_size = cfg.batch_size;
        let total = total.clone();
        let start = start.clone();

        tasks.spawn(async move {
            let mut conn = pool.get_conn().await?;
            let mut inserted: u64 = 0;

            while inserted < quota {
                let remaining = (quota - inserted) as usize;
                let this_batch = remaining.min(batch_size);
                let rows = generator.next_batch(this_batch);
                let (sql, params) = build_mysql_insert(&rows);
                conn.exec_drop(sql, params).await?;
                inserted += rows.len() as u64;

                let prev = total.fetch_add(rows.len() as u64, Ordering::Relaxed);
                let new_total = prev + rows.len() as u64;
                if new_total / 100_000 != prev / 100_000 {
                    let rps = new_total as f64 / start.elapsed().as_secs_f64().max(0.001);
                    tracing::info!("MySQL 已插入 {} 行, {:.2} rows/s", new_total, rps);
                }
            }

            conn.disconnect().await?;
            Ok::<(), anyhow::Error>(())
        });
    }

    while let Some(res) = tasks.join_next().await {
        res??;
    }

    let total_inserted = total.load(Ordering::Relaxed);
    tracing::info!(
        "MySQL 装载完成，总行数 {}，耗时 {:.2}s",
        total_inserted,
        start.elapsed().as_secs_f64()
    );
    {
        let mut conn = pool.get_conn().await?;
        conn.query_drop("ANALYZE TABLE events").await?;
        conn.disconnect().await?;
    }
    pool.disconnect().await?;
    Ok(())
}

async fn load_postgres(url: &str, cfg: &LoadConfig, remaining: u64, _gen: &mut EventGenerator) -> Result<()> {
    let (client, connection) = tokio_postgres::connect(url, tokio_postgres::NoTls).await?;
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            tracing::error!("Postgres 连接任务出错: {}", e);
        }
    });
    configure_postgres_indexes(&client, cfg.indexes).await?;

    let workers = cfg.concurrency.max(1).min(remaining as usize);
    let base_quota = remaining / workers as u64;
    let remainder = remaining % workers as u64;
    let total = Arc::new(AtomicU64::new(0));
    let start = Instant::now();

    let mut tasks = JoinSet::new();
    for worker_id in 0..workers {
        let quota = base_quota + if worker_id < remainder as usize { 1 } else { 0 };
        let mut generator =
            EventGenerator::with_seed(cfg.distribution, cfg.payload_size, worker_id as u64 + 1);
        let url = url.to_string();
        let batch_size = cfg.batch_size;
        let total = total.clone();
        let start = start.clone();

        tasks.spawn(async move {
            let (client, connection) = tokio_postgres::connect(&url, tokio_postgres::NoTls).await?;
            tokio::spawn(async move {
                if let Err(e) = connection.await {
                    tracing::error!("Postgres worker 连接任务出错: {}", e);
                }
            });

            let mut inserted: u64 = 0;
            while inserted < quota {
                let remaining = (quota - inserted) as usize;
                let this_batch = remaining.min(batch_size);
                let rows = generator.next_batch(this_batch);
                let (sql, params) = build_postgres_insert(&rows);
                let param_refs: Vec<&(dyn ToSql + Sync)> = params
                    .iter()
                    .map(|p| p.as_ref() as &(dyn ToSql + Sync))
                    .collect();
                client.execute(sql.as_str(), &param_refs).await?;
                inserted += rows.len() as u64;

                let prev = total.fetch_add(rows.len() as u64, Ordering::Relaxed);
                let new_total = prev + rows.len() as u64;
                if new_total / 100_000 != prev / 100_000 {
                    let rps = new_total as f64 / start.elapsed().as_secs_f64().max(0.001);
                    tracing::info!("Postgres 已插入 {} 行, {:.2} rows/s", new_total, rps);
                }
            }

            Ok::<(), anyhow::Error>(())
        });
    }

    while let Some(res) = tasks.join_next().await {
        res??;
    }

    let total_inserted = total.load(Ordering::Relaxed);
    tracing::info!(
        "Postgres 装载完成，总行数 {}，耗时 {:.2}s",
        total_inserted,
        start.elapsed().as_secs_f64()
    );
    client.execute("ANALYZE events", &[]).await?;
    Ok(())
}

fn build_mysql_insert(rows: &[EventRow]) -> (String, MyParams) {
    let mut placeholders = Vec::with_capacity(rows.len());
    let mut values: Vec<MyValue> = Vec::with_capacity(rows.len() * 6);

    for row in rows {
        placeholders.push("(?, ?, ?, ?, ?, ?)".to_string());
        values.push(MyValue::Int(row.user_id));

        values.push(MyValue::Date(
            row.created_at.year() as u16,
            row.created_at.month() as u8,
            row.created_at.day() as u8,
            row.created_at.hour() as u8,
            row.created_at.minute() as u8,
            row.created_at.second() as u8,
            row.created_at.timestamp_subsec_micros(),
        ));

        values.push(MyValue::Bytes(format!("{:.2}", row.amount).into_bytes()));
        values.push(MyValue::Int(row.status as i64));
        values.push(MyValue::Int(row.category as i64));
        values.push(MyValue::Bytes(row.payload.clone().into_bytes()));
    }

    let sql = format!(
        "INSERT INTO events (user_id, created_at, amount, status, category, payload) VALUES {}",
        placeholders.join(",")
    );
    (sql, MyParams::Positional(values))
}

fn build_postgres_insert(rows: &[EventRow]) -> (String, Vec<Box<dyn ToSql + Send + Sync>>) {
    let mut placeholders = Vec::with_capacity(rows.len());
    let mut params: Vec<Box<dyn ToSql + Send + Sync>> = Vec::with_capacity(rows.len() * 6);
    let mut idx = 1;

    for row in rows {
        placeholders.push(format!(
            "(${},{},{},{},{},{})",
            idx,
            idx + 1,
            idx + 2,
            idx + 3,
            idx + 4,
            idx + 5
        ));
        idx += 6;
        params.push(Box::new(row.user_id));
        params.push(Box::new(row.created_at));
        params.push(Box::new(row.amount));
        params.push(Box::new(row.status));
        params.push(Box::new(row.category));
        params.push(Box::new(row.payload.clone()));
    }

    let sql = format!(
        "INSERT INTO events (user_id, created_at, amount, status, category, payload) VALUES {}",
        placeholders.join(",")
    );

    (sql, params)
}

async fn configure_mysql_indexes(conn: &mut MyConn, mode: IndexMode) -> Result<()> {
    match mode {
        IndexMode::On => {
            ensure_mysql_index(
                conn,
                "idx_user_created",
                "ALTER TABLE events ADD INDEX idx_user_created (user_id, created_at)",
            )
            .await?;
            ensure_mysql_index(
                conn,
                "idx_status",
                "ALTER TABLE events ADD INDEX idx_status (status)",
            )
            .await?;
            ensure_mysql_index(
                conn,
                "idx_created_at",
                "ALTER TABLE events ADD INDEX idx_created_at (created_at)",
            )
            .await?;
            tracing::info!("MySQL 索引已开启");
        }
        IndexMode::Off => {
            drop_mysql_index(conn, "idx_user_created").await?;
            drop_mysql_index(conn, "idx_status").await?;
            drop_mysql_index(conn, "idx_created_at").await?;
            tracing::info!("MySQL 索引已关闭（仅保留主键）");
        }
    }
    Ok(())
}

async fn ensure_mysql_index(conn: &mut MyConn, name: &str, create_sql: &str) -> Result<()> {
    if mysql_index_exists(conn, name).await? {
        return Ok(());
    }
    conn.exec_drop(create_sql, ()).await?;
    Ok(())
}

async fn drop_mysql_index(conn: &mut MyConn, name: &str) -> Result<()> {
    if !mysql_index_exists(conn, name).await? {
        return Ok(());
    }
    let sql = format!("DROP INDEX {} ON events", name);
    conn.exec_drop(sql, ()).await?;
    Ok(())
}

async fn mysql_index_exists(conn: &mut MyConn, name: &str) -> Result<bool> {
    let count: Option<u64> = conn
        .exec_first(
            "SELECT COUNT(1) FROM information_schema.statistics WHERE table_schema = DATABASE() AND table_name = 'events' AND index_name = ?",
            (name,),
        )
        .await?;
    Ok(count.unwrap_or(0) > 0)
}

async fn configure_postgres_indexes(client: &PgClient, mode: IndexMode) -> Result<()> {
    match mode {
        IndexMode::On => {
            client
                .execute(
                    "CREATE INDEX IF NOT EXISTS idx_user_created ON public.events (user_id, created_at)",
                    &[],
                )
                .await?;
            client
                .execute(
                    "CREATE INDEX IF NOT EXISTS idx_status ON public.events (status)",
                    &[],
                )
                .await?;
            client
                .execute(
                    "CREATE INDEX IF NOT EXISTS idx_created_at ON public.events (created_at)",
                    &[],
                )
                .await?;
            tracing::info!("Postgres 索引已开启");
        }
        IndexMode::Off => {
            client
                .execute("DROP INDEX IF EXISTS idx_user_created", &[])
                .await?;
            client.execute("DROP INDEX IF EXISTS idx_status", &[]).await?;
            client
                .execute("DROP INDEX IF EXISTS idx_created_at", &[])
                .await?;
            tracing::info!("Postgres 索引已关闭（仅保留主键）");
        }
    }
    Ok(())
}

pub async fn fetch_mysql_max_id(pool: &MyPool) -> Result<u64> {
    let mut conn = pool.get_conn().await?;
    let max_id: Option<u64> = conn
        .exec_first("SELECT MAX(id) FROM events", ())
        .await?;
    conn.disconnect().await?;
    Ok(max_id.unwrap_or(0))
}

pub async fn fetch_postgres_max_id(client: &PgClient) -> Result<u64> {
    let row = client
        .query_opt("SELECT MAX(id) FROM events", &[])
        .await?;
    if let Some(row) = row {
        let value: Option<i64> = row.get(0);
        Ok(value.unwrap_or(0).max(0) as u64)
    } else {
        Ok(0)
    }
}

pub async fn count_mysql_rows(url: &str) -> Result<u64> {
    let pool = MyPool::new(mysql_async::Opts::from_url(url)?);
    let mut conn = pool.get_conn().await?;
    let count: Option<u64> = conn.exec_first("SELECT COUNT(*) FROM events", ()).await?;
    conn.disconnect().await?;
    Ok(count.unwrap_or(0))
}

pub async fn count_postgres_rows(url: &str) -> Result<u64> {
    let (client, connection) = tokio_postgres::connect(url, tokio_postgres::NoTls).await?;
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            tracing::error!("Postgres 连接任务出错: {}", e);
        }
    });
    let row = client
        .query_opt("SELECT COUNT(*) FROM events", &[])
        .await?;
    let count = row
        .and_then(|r| r.get::<usize, Option<i64>>(0))
        .unwrap_or(0)
        .max(0) as u64;
    Ok(count)
}
