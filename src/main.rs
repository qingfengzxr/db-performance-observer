use std::path::PathBuf;

use anyhow::Result;
use clap::{value_parser, ArgAction, Args, Parser, Subcommand};
use tracing::Level;
use tracing_subscriber::EnvFilter;

mod bench;
mod config;
mod generator;
mod load;

use config::{default_url, DbConfig, DbKind, Distribution, IndexMode};
use load::LoadConfig;

#[derive(Parser, Debug)]
#[command(author, version, about = "DB performance observer CLI")]
struct Cli {
    /// Database kind to target (mysql or postgres)
    #[arg(long, value_enum, default_value_t = DbKind::Mysql)]
    db: DbKind,

    /// Database connection URL. If omitted, a sensible default per DB kind is used.
    #[arg(long)]
    url: Option<String>,

    /// Enable verbose logging
    #[arg(long, short = 'v', action = ArgAction::Count)]
    verbose: u8,

    /// Subcommand to execute
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Generate and load data into the target database
    Load(LoadArgs),
    /// Run benchmark scenarios against the target database
    Bench(BenchArgs),
}

#[derive(Args, Debug)]
struct LoadArgs {
    /// Target row count (e.g. 1000000 for 1m)
    #[arg(long, value_parser = value_parser!(u64).range(1..))]
    scale: u64,
    /// Concurrent workers generating/loading data
    #[arg(long, default_value_t = 4)]
    concurrency: usize,
    /// Rows per batch insert/COPY
    #[arg(long, default_value_t = 10_000)]
    batch_size: usize,
    /// Distribution of user_id values
    #[arg(long, value_enum, default_value_t = Distribution::Uniform)]
    distribution: Distribution,
    /// Payload length for the payload column
    #[arg(long, default_value_t = 200)]
    payload_size: usize,
    /// Whether secondary indexes should exist during load/bench
    #[arg(long, value_enum, default_value_t = IndexMode::On)]
    indexes: IndexMode,
}

#[derive(Args, Debug)]
struct BenchArgs {
    /// Number of warmup operations per scenario
    #[arg(long, default_value_t = 1000)]
    warmup_ops: u64,
    /// Number of measured operations per scenario
    #[arg(long, default_value_t = 10_000)]
    sample_ops: u64,
    /// Maximum concurrent benchmark tasks
    #[arg(long, default_value_t = 16)]
    concurrency: usize,
    /// Output JSON file to write benchmark summary
    #[arg(long)]
    output: Option<PathBuf>,
    /// RNG seed to make benchmark parameters可复现
    #[arg(long, default_value_t = 42)]
    seed: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    init_tracing(cli.verbose)?;

    let db = DbConfig {
        kind: cli.db,
        url: cli.url.unwrap_or_else(|| default_url(cli.db)),
    };

    match cli.command {
        Command::Load(args) => {
            let cfg = LoadConfig {
                scale: args.scale,
                concurrency: args.concurrency,
                batch_size: args.batch_size,
                distribution: args.distribution,
                payload_size: args.payload_size,
                indexes: args.indexes,
            };
            load::run_load(db, cfg).await?;
        }
        Command::Bench(args) => {
            let cfg = bench::BenchConfig {
                warmup_ops: args.warmup_ops,
                sample_ops: args.sample_ops,
                concurrency: args.concurrency,
                output: args.output,
                seed: args.seed,
            };
            bench::run_bench(db, cfg).await?;
        }
    }

    Ok(())
}

fn init_tracing(verbose: u8) -> Result<()> {
    let level = match verbose {
        0 => Level::INFO,
        1 => Level::DEBUG,
        _ => Level::TRACE,
    };

    let filter = EnvFilter::builder()
        .with_default_directive(level.into())
        .from_env_lossy();

    tracing_subscriber::fmt().with_env_filter(filter).init();
    Ok(())
}
