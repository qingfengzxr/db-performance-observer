#!/usr/bin/env bash
set -euo pipefail

# 初始化数据库 schema。支持 mysql 或 postgres（默认 mysql）。

DB_TYPE="${1:-mysql}"

case "$DB_TYPE" in
  mysql)
    echo "初始化 MySQL schema..."
    docker compose exec -T mysql mysql -uroot -proot < init/mysql/01_schema.sql
    ;;
  postgres|pg)
    echo "初始化 Postgres schema..."
    docker compose exec -T postgres psql -U perf -d perf -f /docker-entrypoint-initdb.d/01_schema.sql
    ;;
  *)
    echo "未知 DB_TYPE: $DB_TYPE (支持 mysql|postgres)"
    exit 1
    ;;
esac

echo "完成。"
