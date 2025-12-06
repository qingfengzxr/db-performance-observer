# db-performance-obvser 计划

## 里程碑
1) **环境与 Schema**
   - 提供 docker-compose（MySQL/PostgreSQL 可选）与环境变量。
   - 编写 init SQL：创建 `events` 表和索引；支持带/不带二级索引。
2) **Rust CLI 框架**
   - 增加 CLI 参数与子命令：`load`（装载）和 `bench`（基准）。
   - 封装 MySQL/PostgreSQL 连接与通用配置解析。
3) **数据装载器**
   - 实现数据分布与 payload 长度配置。
   - 实现批量装载：Postgres COPY；MySQL LOAD DATA 或批量 INSERT。
   - 加入装载速率、错误统计与行数校验。
4) **基准场景**
   - 覆盖：PK 点查、user_id 点查、created_at 范围、ORDER BY/LIMIT 分页，选做 UPDATE/DELETE。
   - 预热 + 计时，收集吞吐与延迟分位，输出 CSV/JSON。
5) **编排脚本**
   - Shell 脚本按规模（1m→5000m）循环：起库→建表→装载→基准→保存结果到 `results/{db}/{scale}/...`。
   - 支持指定仅重跑某个规模或热/冷跑。
6) **文档与默认值**
   - 记录配置、调优基线与使用示例。
   - 固定一套可复现的 compose 默认（端口、账号、资源）。

## 执行顺序
- 先完成 1–2 得到可运行骨架。
- 实现装载器（3），再补基准场景（4）。
- 装载与基准可用后，添加编排脚本（5）。
- 最后完善文档与默认值（6），并在至少一个小规模上验证。

## 待定事项
- 基线数据库版本（如 MySQL 8.0.x、Postgres 16.x）。
- 默认调优参数（缓存/日志/检查点）。
- 初始跑是否包含宽行 payload 场景，或作为可选开关。
