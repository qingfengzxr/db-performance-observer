# 使用说明（初版）

## 启动数据库
```bash
docker-compose up -d mysql postgres
```
默认账号密码：`perf / perf`，数据库 `perf`。初始表结构和索引已在 `init/mysql`、`init/postgres` 下。

如需在已启动容器上重新初始化 schema，可运行：
```bash
./scripts/init_db.sh mysql
# 或
./scripts/init_db.sh postgres
```

## 装载数据
示例：向 MySQL 装 100 万行，批大小 10k，均匀分布：
```bash
cargo run --release -- --db mysql load --scale 1000000
```
向 Postgres 装 500 万行，Zipf 分布、payload 100 字节：
```bash
cargo run --release -- --db postgres load --scale 5000000 --distribution zipf --payload-size 100
```
可选参数：
- `--url` 自定义连接串，默认 MySQL `mysql://perf:perf@127.0.0.1:3306/perf`，Postgres `postgres://perf:perf@127.0.0.1:5432/perf`。
- `--batch-size` 每批行数（默认 10k）。
- `--concurrency` 并发生成/写入的 worker 数（默认 4）。
- `--indexes on|off` 索引开关：装载前会创建/删除二级索引（主键保留）。

## 基准测试
示例：对 MySQL 跑预设查询场景，4 并发，预热 500，采样 2000，输出到文件：
```bash
cargo run --release -- --db mysql bench --warmup-ops 500 --sample-ops 2000 --concurrency 4 --output results-mysql.json
```
输出为 JSON（包含场景名、吞吐、p50/p95/p99）。

预设场景：
- `pk_hit`: 通过主键点查。
- `user_lookup`: 按 user_id 查最近一条。
- `range_small`: 最近 1 天范围，ORDER BY created_at LIMIT 50。
- `range_large`: 最近 30 天范围，ORDER BY created_at LIMIT 200。
- `order_page`: ORDER BY created_at，LIMIT 50 OFFSET 100。
基准时每完成 500 次采样会输出一次进度，包含场景名与当前吞吐。

## 一键跑完整流程
使用脚本自动启动容器、初始化 schema、按规模循环装载+基准，结果输出到 `results/{db}/{scale}/`：
```bash
./scripts/run_all.sh --db mysql --scales "1000000 5000000" --concurrency 8 --bench-concurrency 16
```
可选参数：
- `--db mysql|postgres`
- `--scales "1000000 5000000 10000000"`：以空格分隔的行数
- `--concurrency` / `--batch-size`：装载并发和批大小
- `--bench-concurrency` / `--warmup-ops` / `--sample-ops` / `--bench-seed`
- `--indexes on|off`
- `--results DIR`
- `--cold-bench`：基准前重启容器，模拟冷缓存（会清空 buffer/cache）

脚本会：
1) `docker compose up -d {db}`；
2) 调用 `scripts/init_db.sh` 重建 schema；
3) 逐规模执行 `cargo run -- load ...` 和 `cargo run -- bench ...`，日志保存到 `results/.../load.log`、`bench.log`，基准结果为 `bench.json`。
4) 若有 python3，自动将 `bench.json` 转成 Markdown 表格 `bench.md`。
5) 若安装了 matplotlib，自动汇总所有规模的 `bench.json`，生成 `summary.md` 和按场景的吞吐/p99 折线图（`results/{db}/summary/`）。

## 注意
- 当前装载实现使用批量 INSERT，适合先验证流程；大规模跑数后续会增加 COPY/LOAD DATA 优化。
- 装载过程中会输出累计行数与速率。
