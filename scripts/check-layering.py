#!/usr/bin/env python3
"""Enforces the crate dependency rules from docs/architecture.md section 3.

Run from the repository root: python3 -I scripts/check-layering.py
"""
import json
import subprocess
import sys

meta = json.loads(subprocess.check_output(
    ["cargo", "metadata", "--format-version", "1", "--no-deps"], text=True))
deps = {p["name"]: {d["name"] for d in p["dependencies"] if d["kind"] != "dev"} for p in meta["packages"]}
all_deps = {p["name"]: {d["name"] for d in p["dependencies"]} for p in meta["packages"]}
yaac = {n for n in deps if n.startswith("yaac-")}

errors = []


def forbid(crate, names, why, include_dev=False):
    src = all_deps if include_dev else deps
    for n in sorted(src.get(crate, set())):
        if n in names or any(n.startswith(p) for p in names if p.endswith("*")):
            errors.append(f"{crate} must not depend on {n}: {why}")


# yaac-model depends on no other yaac crate.
forbid("yaac-model", yaac, "the model is pure", include_dev=True)
# yaac-core sees traits only.
forbid("yaac-core", {"cef", "rusqlite", "yaac-store", "yaac-cef", "yaac-platform", "objc2*", "yaac-app"},
       "the core names no engine, OS or SQLite crate")
# Only yaac-cef depends on cef; only yaac-platform and yaac-cef depend on OS crates.
for crate, d in deps.items():
    if "cef" in d and crate != "yaac-cef":
        errors.append(f"{crate} depends on cef: only yaac-cef may")
    if any(n.startswith("objc2") for n in d) and crate not in ("yaac-platform", "yaac-cef"):
        errors.append(f"{crate} depends on objc2: only yaac-platform and yaac-cef may")
# The CLI client reaches state only through the wire types.
forbid("yaac-cli", yaac - {"yaac-api", "yaac-model"}, "the client depends on the wire types only")
# Adapters depend on core and model, never the reverse.
for adapter in ("yaac-store", "yaac-cef", "yaac-platform"):
    for crate in ("yaac-model", "yaac-api", "yaac-engine", "yaac-core"):
        if adapter in deps.get(crate, set()):
            errors.append(f"{crate} must not depend on adapter {adapter}")

if errors:
    print("layering violations:")
    for e in errors:
        print(" -", e)
    sys.exit(1)
print("layering ok")
