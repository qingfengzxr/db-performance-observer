#!/usr/bin/env python3
"""
汇总多个规模的 bench.json，生成 Markdown 汇总表和图表（需要 matplotlib）。
用法:
  python scripts/plot_results.py --results results --db mysql --output results/mysql/summary
"""

import argparse
import json
import sys
from pathlib import Path
from typing import Dict, List, Any


def load_bench(path: Path) -> List[Dict[str, Any]]:
    with path.open("r", encoding="utf-8") as f:
        return json.load(f)


def collect(results_dir: Path, db: str) -> Dict[str, Dict[str, Any]]:
    root = results_dir / db
    data: Dict[str, Dict[str, Any]] = {}
    for scale_dir in sorted(root.iterdir()):
        if not scale_dir.is_dir():
            continue
        bench = scale_dir / "bench.json"
        if not bench.exists():
            continue
        try:
            rows = load_bench(bench)
        except Exception as e:
            print(f"[WARN] 读取 {bench} 失败: {e}", file=sys.stderr)
            continue
        data[scale_dir.name] = {row["scenario"]: row for row in rows}
    return data


def ensure_output(dir_path: Path) -> None:
    dir_path.mkdir(parents=True, exist_ok=True)


def write_markdown(data: Dict[str, Dict[str, Any]], output_md: Path) -> None:
    # 场景列表取第一条记录的 key 集合
    scenarios = set()
    for rows in data.values():
        scenarios.update(rows.keys())
    scenarios = sorted(scenarios)

    lines = []
    for scenario in scenarios:
        lines.append(f"## {scenario}")
        lines.append("| scale | ops | throughput_ops | p50_ms | p95_ms | p99_ms |")
        lines.append("| --- | --- | --- | --- | --- | --- |")
        for scale in sorted(data.keys(), key=lambda x: int(x)):
            row = data[scale].get(scenario, {})
            lines.append(
                f"| {scale} | {row.get('ops','')} | "
                f"{row.get('throughput_ops',0):.2f} | "
                f"{row.get('p50_ms',0):.3f} | "
                f"{row.get('p95_ms',0):.3f} | "
                f"{row.get('p99_ms',0):.3f} |"
            )
        lines.append("")
    output_md.write_text("\n".join(lines), encoding="utf-8")
    print(f"Wrote summary markdown to {output_md}")


def plot(data: Dict[str, Dict[str, Any]], output_dir: Path) -> None:
    try:
        import matplotlib.pyplot as plt
    except Exception as e:
        print(f"[WARN] 无法导入 matplotlib，跳过绘图: {e}", file=sys.stderr)
        return

    scenarios = set()
    for rows in data.values():
        scenarios.update(rows.keys())
    scenarios = sorted(scenarios)
    scales = sorted(data.keys(), key=lambda x: int(x))

    # Throughput chart per scenario
    for scenario in scenarios:
        fig, ax = plt.subplots(figsize=(8, 4))
        vals = []
        for scale in scales:
            row = data[scale].get(scenario)
            vals.append(row["throughput_ops"] if row else 0)
        ax.plot(scales, vals, marker="o")
        ax.set_title(f"Throughput - {scenario}")
        ax.set_xlabel("scale (rows)")
        ax.set_ylabel("ops/sec")
        ax.grid(True, linestyle="--", alpha=0.4)
        fig.tight_layout()
        out_path = output_dir / f"{scenario}_throughput.png"
        fig.savefig(out_path, dpi=150)
        plt.close(fig)
        print(f"Wrote {out_path}")

    # p99 latency chart per scenario
    for scenario in scenarios:
        fig, ax = plt.subplots(figsize=(8, 4))
        vals = []
        for scale in scales:
            row = data[scale].get(scenario)
            vals.append(row["p99_ms"] if row else 0)
        ax.plot(scales, vals, marker="o", color="tomato")
        ax.set_title(f"P99 latency (ms) - {scenario}")
        ax.set_xlabel("scale (rows)")
        ax.set_ylabel("p99 (ms)")
        ax.grid(True, linestyle="--", alpha=0.4)
        fig.tight_layout()
        out_path = output_dir / f"{scenario}_p99.png"
        fig.savefig(out_path, dpi=150)
        plt.close(fig)
        print(f"Wrote {out_path}")


def main() -> None:
    parser = argparse.ArgumentParser(description="汇总 bench.json 输出图表和汇总表。")
    parser.add_argument("--results", default="results", help="结果根目录，默认 results")
    parser.add_argument("--db", default="mysql", help="数据库类型目录名，默认 mysql")
    parser.add_argument("--output", required=True, help="输出目录，用于汇总图表/markdown")
    args = parser.parse_args()

    results_dir = Path(args.results)
    output_dir = Path(args.output)
    ensure_output(output_dir)

    data = collect(results_dir, args.db)
    if not data:
        print(f"[WARN] 未找到任何 bench.json (路径 {results_dir}/{args.db})", file=sys.stderr)
        sys.exit(0)

    write_markdown(data, output_dir / "summary.md")
    plot(data, output_dir)


if __name__ == "__main__":
    main()
