# db-performance-obvser 设计

## 目标
- 测量 MySQL/PostgreSQL 在不同规模（1m/5m/10m/50m/100m/500m/1000m/2000m/5000m）下的性能。
- 比较有索引/无索引的常见 OLTP 访问模式。
- 提供可重复的脚本化跑数，输出吞吐、延迟（p50/p95/p99）到 CSV/JSON。

## 系统概览
组件：
1. **容器启动**：`docker-compose.yml` 或 `docker run` 包装脚本，可参数化选择 MySQL/PostgreSQL 版本、端口、账号、数据卷，附带初始化 SQL。
2. **Schema/init SQL**：创建 `events` 表和索引；可切换是否创建二级索引。
3. **数据生成器（Rust）**：并发生成不同规模数据，支持 COPY/LOAD DATA 或批量 INSERT，配置数据分布。
4. **基准执行器（Rust）**：运行查询/写入场景，做预热与采样，记录指标，结构化输出。
5. **编排脚本**：Shell 脚本按规模循环执行装库与基准，管理结果目录。

## 表结构
```sql
CREATE TABLE events (
  id BIGINT PRIMARY KEY AUTO_INCREMENT, -- Postgres 用 BIGSERIAL
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
可选变体：
- 窄行：`payload` 改为 `VARCHAR(50)` 看行宽影响。
- 宽行：`payload` 加大或 TEXT/JSONB。
- 索引开关：删除 `idx_user_created` 或 `idx_status` 强制走全表扫。

数据分布（可配置）：
- `user_id`：均匀或热点（Zipf）。
- `status`：低基数 0–4。
- `category`：中基数（如 0–5000）。
- `created_at`：最近一段时间均匀分布。
- `amount`：限定范围随机。

## 数据装载
- Rust CLI 子命令 `load`：参数化规模、库类型、批大小、并发度、分布、payload 长度。
- 优先批量：Postgres 用 COPY；MySQL 用 LOAD DATA LOCAL INFILE，或多行 INSERT（5k–20k 一批）。
- 并发：按分片生成流式写库，错误回压；记录行速率和失败。

## 基准测试
- Rust CLI 子命令 `bench`：针对已装库运行场景：
  - PK 点查（命中/未命中）。
  - `user_id` 索引点查（有/无匹配）。
  - `created_at` 范围查询（小/中/大窗口）。
  - ORDER BY created_at LIMIT/OFFSET 分页。
  - 可选：按索引条件 UPDATE/DELETE。
- 每个场景：预热 N 次，计时 M 次；记录吞吐与延迟分位。
- 输出：CSV/JSON，包含配置元数据（规模、库类型、索引状态、种子、硬件备注）。

## 编排
- `docker-compose.yml` 用 env 控制库类型、端口、账号、资源限制，附 `launch.sh` 启停并应用初始化 SQL。
- `run_all.sh` 迭代各规模：建库/建表（含/不含索引）→ 装载 → 基准（热/冷）→ 存入 `results/{db}/{scale}/...`。
- 支持通过参数仅重跑某个规模。

## 配置
- CLI 主要参数：`--db {mysql,postgres}`、`--scale`、`--batch-size`、`--concurrency`、`--distribution`、`--payload-size`、`--indexes on|off`。
- Compose 环境：端口、用户密码、DB 名、资源限制、版本。
- 调优基线：MySQL（InnoDB buffer pool/log size）、Postgres（shared_buffers、WAL/checkpoint）固定成一套默认值。

## 指标与校验
- 收集：装载行速率、总耗时、错误；基准吞吐和延迟分位。
- 可选：场景跑前抓 `EXPLAIN ANALYZE` 确认走索引/全表。
- 校验：装载后计数对齐；检查预期索引是否存在。

## 扩展目标
- 增加 ClickHouse 做列式对比。
- 接入 Prometheus/Grafana 采集 CPU/内存/IO。
- 支持多种 payload 画像（文本 vs JSON）同库跑。
