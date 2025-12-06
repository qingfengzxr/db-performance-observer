#!/usr/bin/env bash
set -euo pipefail

DB_TYPE="mysql"
SCALES=(1000000 5000000 10000000)
CONCURRENCY=4
BATCH_SIZE=10000
WARMUP_OPS=1000
SAMPLE_OPS=5000
INDEXES="on"
RESULTS_DIR="results"
BENCH_CONCURRENCY=8
BENCH_SEED=42
COLD_BENCH=false

die() { echo "[ERROR] $*" >&2; exit 1; }

usage() {
  cat <<USAGE
用法: $0 [选项]
  --db mysql|postgres      选择数据库类型，默认 mysql
  --scales "1 5 10"        以空格分隔的规模（行数），单位为行（整数），默认 1m/5m/10m
  --concurrency N          装载并发 worker 数，默认 4
  --bench-concurrency N    基准并发 worker 数，默认 8
  --batch-size N           装载批大小，默认 10000
  --indexes on|off         是否保留二级索引，默认 on
  --warmup-ops N           基准预热次数，默认 1000
  --sample-ops N           基准采样次数，默认 5000
  --bench-seed N           基准随机种子，默认 42
  --results DIR            结果目录，默认 results
  --cold-bench             基准前重启容器，模拟冷缓存
  --help                   打印帮助
示例：
  $0 --db mysql --scales "1000000 5000000" --concurrency 8 --bench-concurrency 16
USAGE
}

parse_args() {
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --db) DB_TYPE="$2"; shift 2;;
      --scales) IFS=' ' read -r -a SCALES <<< "$2"; shift 2;;
      --concurrency) CONCURRENCY="$2"; shift 2;;
      --bench-concurrency) BENCH_CONCURRENCY="$2"; shift 2;;
      --batch-size) BATCH_SIZE="$2"; shift 2;;
      --indexes) INDEXES="$2"; shift 2;;
      --warmup-ops) WARMUP_OPS="$2"; shift 2;;
      --sample-ops) SAMPLE_OPS="$2"; shift 2;;
      --bench-seed) BENCH_SEED="$2"; shift 2;;
      --results) RESULTS_DIR="$2"; shift 2;;
      --cold-bench) COLD_BENCH=true; shift 1;;
      --help) usage; exit 0;;
      *) die "未知参数: $1";;
    esac
  done
}

ensure_compose() {
  docker compose up -d "$DB_TYPE"
  if [[ "$DB_TYPE" == "mysql" ]]; then
    wait_mysql
  else
    wait_postgres
  fi
}

wait_mysql() {
  while true; do
    if docker compose exec -T mysql mysqladmin -h127.0.0.1 -uroot -proot ping >/dev/null 2>&1; then
      return 0
    fi
    echo "[INFO] MySQL 未就绪，30s 后重试..."
    sleep 30
  done
}

wait_postgres() {
  while true; do
    if docker compose exec -T postgres pg_isready -U perf -d perf >/dev/null 2>&1; then
      return 0
    fi
    echo "[INFO] Postgres 未就绪，30s 后重试..."
    sleep 30
  done
}

init_schema() {
  if [[ "$DB_TYPE" == "mysql" ]]; then
    ./scripts/init_db.sh mysql
  else
    ./scripts/init_db.sh postgres
  fi
}

run_scale() {
  local scale="$1"
  local out_dir="$RESULTS_DIR/$DB_TYPE/$scale"
  mkdir -p "$out_dir"

  echo "=== 装载 ${scale} 行 (${DB_TYPE}) ==="
  cargo run --release -- --db "$DB_TYPE" load --scale "$scale" \
    --concurrency "$CONCURRENCY" --batch-size "$BATCH_SIZE" --indexes "$INDEXES" \
    2>&1 | tee "$out_dir/load.log"

  if $COLD_BENCH; then
    echo "=== 冷缓存模式：重启 ${DB_TYPE} 容器，准备基准 ==="
    docker compose restart "$DB_TYPE"
    if [[ "$DB_TYPE" == "mysql" ]]; then
      wait_mysql
    else
      wait_postgres
    fi
  fi

  echo "=== 基准 ${scale} 行 (${DB_TYPE}) ==="
  cargo run --release -- --db "$DB_TYPE" bench \
    --concurrency "$BENCH_CONCURRENCY" --warmup-ops "$WARMUP_OPS" --sample-ops "$SAMPLE_OPS" \
    --seed "$BENCH_SEED" \
    --output "$out_dir/bench.json" 2>&1 | tee "$out_dir/bench.log"

  if command -v python3 >/dev/null 2>&1; then
    python3 scripts/format_results.py --input "$out_dir/bench.json" --output "$out_dir/bench.md" || \
      echo "[WARN] 转换 Markdown 失败（python3 脚本执行错误）"
  else
    echo "[WARN] 未找到 python3，跳过 Markdown 表格生成"
  fi
}

main() {
  parse_args "$@"
  ensure_compose
  init_schema
  for s in "${SCALES[@]}"; do
    run_scale "$s"
  done

  if command -v python3 >/dev/null 2>&1; then
    python3 scripts/plot_results.py --results "$RESULTS_DIR" --db "$DB_TYPE" --output "$RESULTS_DIR/$DB_TYPE/summary" || \
      echo "[WARN] 汇总图表生成失败（可能缺少 matplotlib）"
  else
    echo "[WARN] 未找到 python3，跳过汇总图表生成"
  fi
  echo "全部完成，结果保存在 $RESULTS_DIR/$DB_TYPE/"
}

main "$@"
