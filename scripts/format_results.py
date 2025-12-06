#!/usr/bin/env python3
"""
将 bench.json 转为 Markdown 表格。
用法:
  python scripts/format_results.py --input results/mysql/1000000/bench.json --output results/mysql/1000000/bench.md
"""

import argparse
import json
from pathlib import Path
from typing import List, Dict, Any


def load_results(path: Path) -> List[Dict[str, Any]]:
    with path.open("r", encoding="utf-8") as f:
        return json.load(f)


def to_markdown(rows: List[Dict[str, Any]]) -> str:
    headers = ["scenario", "ops", "throughput_ops", "p50_ms", "p95_ms", "p99_ms"]
    lines = ["| " + " | ".join(headers) + " |", "|" + "|".join([" --- "] * len(headers)) + "|"]
    for row in rows:
        lines.append(
            "| "
            + " | ".join(
                [
                    str(row.get("scenario", "")),
                    str(row.get("ops", "")),
                    f"{row.get('throughput_ops', 0):.2f}",
                    f"{row.get('p50_ms', 0):.3f}",
                    f"{row.get('p95_ms', 0):.3f}",
                    f"{row.get('p99_ms', 0):.3f}",
                ]
            )
            + " |"
        )
    return "\n".join(lines) + "\n"


def main() -> None:
    parser = argparse.ArgumentParser(description="Convert bench.json to markdown table.")
    parser.add_argument("--input", required=True, help="bench.json path")
    parser.add_argument("--output", required=True, help="output markdown path")
    args = parser.parse_args()

    input_path = Path(args.input)
    output_path = Path(args.output)

    rows = load_results(input_path)
    md = to_markdown(rows)
    output_path.write_text(md, encoding="utf-8")
    print(f"Wrote markdown table to {output_path}")


if __name__ == "__main__":
    main()
