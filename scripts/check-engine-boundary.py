#!/usr/bin/env python3
"""Reject the retired Servo browser backend in either pinned dependency graph.

Stylo and its reusable servo_arc support crates remain permitted.
"""
from pathlib import Path
import tomllib

ROOT = Path(__file__).resolve().parents[1]
FORBIDDEN = {
    "servo", "mozjs", "mozjs_sys", "mozjs-sys", "spidermonkey",
    "servo-script", "servo-script-bindings", "servo-script-traits",
    "servo-constellation", "servo-layout", "servo-layout-2020",
    "servo-net", "servo-storage", "servo-devtools", "servo-devtools-traits",
    "servo-paint", "servo-paint-api", "servo-embedder-traits",
}

def main():
    for relative in ("Cargo.lock", "engines/Cargo.lock"):
        with (ROOT / relative).open("rb") as source:
            names = {package["name"] for package in tomllib.load(source)["package"]}
        retired = sorted(names & FORBIDDEN)
        if retired:
            raise SystemExit(f"{relative}: retired engine dependencies: {retired}")
        print(f"{relative}: no embedded Servo or SpiderMonkey backend ({len(names)} package names)")
    if (ROOT / "src/servo_host.rs").exists() or (ROOT / "vendor/servo").exists():
        raise SystemExit("The retired embedded backend source has been restored")

if __name__ == "__main__":
    main()
