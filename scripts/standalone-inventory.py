#!/usr/bin/env python3
"""Capture a source-only migration inventory; never launches a browser or a test.

IDL declarations and gate attributes are a lexical index, not an IDL compiler or
proof that an interface is exposed. Runtime evidence is deliberately separate.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def git(*args):
    return subprocess.check_output(["git", *args], cwd=ROOT).decode().strip()


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def reference(path, label=None):
    return {"path": label or path.relative_to(ROOT).as_posix(), "sha256": digest(path)}


def uncomment(text):
    # Preserve strings (including URLs) while removing comments from the index.
    return re.sub(
        r'"(?:\\.|[^"\\])*"|/\*.*?\*/|//[^\n]*',
        lambda m: m.group() if m.group().startswith('"') else "\n" * m.group().count("\n"),
        text, flags=re.S,
    )


def config_source(explicit):
    if explicit:
        path = explicit.resolve()
        if not path.is_file():
            raise ValueError("--servo-config must name servo-config 0.7.0 prefs.rs")
        manifest = tomllib.loads((path.parent / "Cargo.toml").read_text())
        if (manifest["package"]["name"], manifest["package"]["version"]) != ("servo-config", "0.7.0"):
            raise ValueError("Expected pinned servo-config 0.7.0")
        return path
    cargo = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo"))
    matches = sorted((cargo / "registry/src").glob("*/servo-config-0.7.0/prefs.rs"))
    if not matches:
        raise ValueError("Pinned servo-config source unavailable; supply --servo-config")
    # Multiple registries must agree, rather than silently auditing another source.
    if len({digest(path) for path in matches}) != 1:
        raise ValueError("Conflicting servo-config sources; supply --servo-config explicitly")
    return matches[0]


def replacement_steps(name):
    """Conservative routing only; the ledger still requires semantic audit."""
    if name.startswith(("WebGL", "WEBGL", "ANGLE", "EXT", "OES", "GPU")):
        return ["19"]
    if name.startswith(("Canvas", "OffscreenCanvas", "ImageBitmap", "ImageData", "Path2D")):
        return ["18"]
    if name.startswith(("IDB", "Cache", "Storage", "Cookie")):
        return ["16"]
    if any(word in name for word in ("Worker", "MessagePort", "MessageChannel", "BroadcastChannel")):
        return ["17"]
    if name.startswith(("Bluetooth", "Gamepad", "XR", "Permission", "Notification", "Geolocation", "WakeLock")):
        return ["21"]
    if name.startswith(("RTC", "MediaDevice", "MediaStream", "InputDevice", "NavigatorUserMedia")):
        return ["20", "21"]
    if any(word in name for word in ("Audio", "Video", "Media", "Track", "Oscillator", "GainNode", "WaveShaper")):
        return ["20"]
    if any(word in name for word in ("Selection", "Range", "Clipboard", "DataTransfer", "DragEvent")):
        return ["12"]
    if name.startswith(("Shadow", "CustomElement", "HTMLSlot")):
        return ["15"]
    if name.startswith(("CSS", "SVG", "Font", "Animation", "Keyframe", "Scroll", "ViewTransition")):
        return ["13"]
    if any(word in name for word in ("Input", "Form", "Select", "Option", "TextArea", "Validity", "File", "Composition", "Pointer", "Touch", "Keyboard")):
        return ["11"]
    if name in ("Window", "WindowProxy", "HTMLIFrameElement", "BrowsingContext"):
        return ["09", "14"]
    return ["08"]


def capture(config):
    manifest = tomllib.loads((ROOT / "Cargo.toml").read_text())
    prefs_text = config.read_text()
    default_block = prefs_text.split("const fn const_default() -> Self", 1)[1].split("\n    }\n", 1)[0]
    defaults = dict(re.findall(r"^\s+(\w+):\s+(true|false),", uncomment(default_block), re.M))
    bool_defaults = {key: value == "true" for key, value in sorted(defaults.items())}
    files = []
    for path in sorted((ROOT / "vendor/servo-script-bindings/webidls").glob("*.webidl")):
        text = path.read_text()
        code = uncomment(text)
        declarations = [
            {"kind": kind, "name": name}
            for kind, name in re.findall(
                r"\b(interface\s+mixin|interface|dictionary|enum|namespace)\s+(\w+)", code
            )
        ]
        gates = sorted(set(re.findall(r'\bPref\s*=\s*"([^"]+)"', code)))
        files.append({
            **reference(path),
            "declarations": declarations,
            "preference_gates": {name: bool_defaults.get(name, "unknown") for name in gates},
            "secure_context_mentioned": bool(re.search(r"\bSecureContext\b", code)),
            "exposure_annotations": sorted(set(re.findall(r"\bExposed\s*=\s*(\([^)]*\)|\w+|\*)", code))),
            "build_conditions": sorted(set(re.findall(r"// skip-unless ([A-Z_]+)", text))),
            "replacement_steps": replacement_steps(path.stem),
            "routing_status": "provisional_family_requires_semantic_audit",
            "runtime_exposure": "not_measured",
        })
    source_paths = sorted({
        ROOT / p for pattern in ("src/*.rs", "engines/wega/src/*.rs", "engines/wega/src/dom/*.js")
        for p in (path.relative_to(ROOT) for path in ROOT.glob(pattern))
    })
    adapters = []
    for name in ("src/engine.rs", "src/permission_adapter.rs", "src/navigation_adapter.rs"):
        path = ROOT / name
        code = uncomment(path.read_text())
        adapters.append({**reference(path), "methods": re.findall(r"\bfn (\w+)\s*\(", code)})
    webdriver = uncomment((ROOT / "src/automation.rs").read_text())
    fixture_files = sorted(
        path for base in (ROOT / "tests/fixtures", ROOT / "engines/wega/tests/fixtures")
        for path in base.rglob("*") if path.is_file()
    )
    pending = [
        "Runtime captures and platform tests deferred by owner until the final test pass.",
        "IDL gates are lexical mentions; member gates, build conditions, globals and function checks need runtime evaluation.",
        "Existing binary provenance is unknown until the frozen source baseline is built and recorded.",
        "Profile migration audit is source-only; no personal profile has been read, copied or modified.",
    ]
    return {
        "schema": 1,
        "kind": "source_inventory",
        "source_commit": git("rev-parse", "HEAD"),
        "branch": git("branch", "--show-current"),
        "source_files": [reference(path) for path in source_paths],
        "manifests": [reference(ROOT / name) for name in (
            "Cargo.toml", "Cargo.lock", "engines/Cargo.toml", "engines/Cargo.lock",
            "vendor/servo/Cargo.toml", "vendor/servo-script-bindings/Cargo.toml",
        )],
        "cargo_features": manifest["features"],
        "servo_dependency": manifest["dependencies"]["servo"],
        "windows_servo_dependency": manifest["target"]["cfg(windows)"]["dependencies"]["servo"],
        "preferences": {
            "source": reference(config, "registry:servo-config/0.7.0/prefs.rs"),
            "boolean_defaults": bool_defaults,
            "browser_overrides": {
                "accessibility_enabled": True,
                "devtools_server_enabled": "--remote-devtools only",
                "dom_notification_enabled": "test-harness builds only",
            },
        },
        "webidl_files": files,
        "servo_adapters": adapters,
        "webdriver_command_mentions": sorted(set(re.findall(r"\bCommand::(\w+)", webdriver))),
        "test_sources": [reference(path) for path in sorted(
            path for base in (ROOT / "tests", ROOT / "engines/wega/tests", ROOT / "src/harness")
            for path in base.glob("*") if path.suffix in (".rs", ".py")
        )],
        "fixtures": [reference(path) for path in fixture_files],
        "runtime_evidence": [],
        "pending": pending,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--servo-config", type=Path)
    parser.add_argument("--output", type=Path, default=ROOT / "docs/validation/standalone/source-inventory.json")
    args = parser.parse_args()
    try:
        inventory = capture(config_source(args.servo_config))
    except (OSError, ValueError, IndexError, KeyError) as error:
        parser.exit(1, f"Cannot capture source inventory: {error}\n")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(inventory, indent=2) + "\n")
    print(f"Recorded {len(inventory['webidl_files'])} IDL files and "
          f"{len(inventory['fixtures'])} fixtures; runtime evidence deferred.")


if __name__ == "__main__":
    main()
