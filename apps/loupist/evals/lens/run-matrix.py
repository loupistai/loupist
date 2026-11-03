#!/usr/bin/env python3
"""Run independent, explicitly planned production evaluation cells."""
import concurrent.futures
import json
from pathlib import Path
import subprocess
import sys

plan = json.loads(Path(sys.argv[1]).read_text())
script = Path(__file__).with_name("run-cell.py")


def run(cell):
    args = [sys.executable, str(script)]
    for key, value in {**plan["settings"], **cell}.items():
        args.extend(["--" + key.replace("_", "-"), str(value)])
    return subprocess.run(args).returncode


with concurrent.futures.ThreadPoolExecutor(max_workers=plan.get("concurrency", 1)) as pool:
    results = list(pool.map(run, plan["cells"]))
print(json.dumps({"cells": len(results), "completed": results.count(0), "exit_codes": results}))
raise SystemExit(0 if all(code == 0 for code in results) else 1)
