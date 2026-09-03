#!/usr/bin/env python3
"""Pydantic conformance harness.

Runs the entire generated pydantic conformance suite in a single process:
loads conformance/generated/manifest.json, imports each group's generated
module, wraps the group's type in a TypeAdapter, and validates every test
case. Writes conformance/results/pydantic.json and prints a summary.
"""

from __future__ import annotations

import importlib.util
import json
import re
import sys
import time
from collections import Counter
from pathlib import Path

CONFORMANCE_DIR = Path(__file__).resolve().parent.parent
GENERATED_DIR = CONFORMANCE_DIR / "generated"
MANIFEST_PATH = GENERATED_DIR / "manifest.json"
RESULTS_PATH = CONFORMANCE_DIR / "results" / "pydantic.json"

try:
    from pydantic import TypeAdapter
except ImportError:
    print("error: pydantic is not installed (activate conformance/python/.venv)", file=sys.stderr)
    sys.exit(1)


def snake_case(group_id: str) -> str:
    s = group_id.replace("-", "_")
    s = re.sub(r"(?<=[a-z0-9])(?=[A-Z])", "_", s)
    return s.lower()


def module_path_for(group: dict) -> Path:
    rel = group.get("files", {}).get("pydantic")
    if rel:
        return GENERATED_DIR / rel
    return GENERATED_DIR / "pydantic" / f"{snake_case(group['id'])}.py"


def main() -> int:
    manifest = json.loads(MANIFEST_PATH.read_text())
    groups = manifest["groups"]

    total = 0
    passed = 0
    failures = []
    by_keyword: dict[str, dict[str, int]] = {}

    start = time.monotonic()

    for group in groups:
        gid = group["id"]
        keyword = group["keyword"]
        tests = group["tests"]
        kw = by_keyword.setdefault(keyword, {"total": 0, "pass": 0})

        def fail_all(reason: str) -> None:
            nonlocal total
            for test in tests:
                total += 1
                kw["total"] += 1
                failures.append({
                    "id": gid,
                    "keyword": keyword,
                    "test": test["description"],
                    "expected": test["valid"],
                    "actual": None,
                    "reason": reason,
                })

        gen_error = group.get("errors", {}).get("pydantic")
        if gen_error:
            fail_all(f"generation error: {gen_error}")
            continue

        mod_path = module_path_for(group)
        mod_name = f"conformance_pydantic.{snake_case(gid)}"
        try:
            spec = importlib.util.spec_from_file_location(mod_name, mod_path)
            if spec is None or spec.loader is None:
                raise ImportError(f"cannot create import spec for {mod_path}")
            module = importlib.util.module_from_spec(spec)
            sys.modules[mod_name] = module
            spec.loader.exec_module(module)
        except BaseException as exc:  # syntax errors, RecursionError, etc.
            fail_all(f"import error: {type(exc).__name__}: {exc}")
            continue

        type_name = group["type_name"]
        if not hasattr(module, type_name):
            fail_all(f"missing attribute: {type_name} not defined in {mod_path.name}")
            continue

        try:
            adapter = TypeAdapter(getattr(module, type_name))
        except BaseException as exc:
            fail_all(f"TypeAdapter error: {type(exc).__name__}: {exc}")
            continue

        for test in tests:
            total += 1
            kw["total"] += 1
            reason = None
            try:
                adapter.validate_python(test["data"])
                actual = True
            except BaseException as exc:
                actual = False
                reason = f"{type(exc).__name__}"
            if actual == test["valid"]:
                passed += 1
                kw["pass"] += 1
            else:
                entry = {
                    "id": gid,
                    "keyword": keyword,
                    "test": test["description"],
                    "expected": test["valid"],
                    "actual": actual,
                }
                if reason:
                    entry["reason"] = reason
                failures.append(entry)

    elapsed = time.monotonic() - start
    failed = total - passed

    RESULTS_PATH.parent.mkdir(parents=True, exist_ok=True)
    RESULTS_PATH.write_text(json.dumps({
        "language": "pydantic",
        "total": total,
        "pass": passed,
        "fail": failed,
        "failures": failures,
        "byKeyword": by_keyword,
    }, indent=2) + "\n")

    pct = (100.0 * passed / total) if total else 0.0
    print(f"pydantic conformance: {passed}/{total} passed ({pct:.1f}%), {failed} failed")
    print(f"groups: {len(groups)}, elapsed: {elapsed:.2f}s")
    print(f"results written to {RESULTS_PATH}")

    fail_counts = Counter(f["keyword"] for f in failures)
    if fail_counts:
        print("\ntop failing keywords:")
        for kw_name, count in fail_counts.most_common(15):
            kw_total = by_keyword[kw_name]["total"]
            print(f"  {kw_name:<25} {count:>4} failed / {kw_total} total")

    return 0


if __name__ == "__main__":
    sys.exit(main())
