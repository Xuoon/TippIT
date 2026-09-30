"""Konfigurationen parsen und die Versionsverträge von Toolchain und Release prüfen."""

import json
from pathlib import Path
import re
import sys
import tomllib

root = Path(__file__).resolve().parents[2]
fehler: list[str] = []


def check(bedingung: bool, meldung: str) -> None:
    # Kein assert: `python -O` würde die Prüfungen still überspringen.
    if not bedingung:
        fehler.append(meldung)


def read(relativ: str) -> str:
    return (root / relativ).read_text(encoding="utf-8")


for path in [*(root / "src-tauri").glob("*.json"), *(root / "src-tauri/capabilities").glob("*.json")]:
    try:
        json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as error:
        fehler.append(f"{path.relative_to(root)}: {error}")

package = json.loads(read("package.json"))
check(bool(re.fullmatch(r"bun@\d+\.\d+\.\d+", package.get("packageManager", ""))), "packageManager muss eine exakte Bun-Version nennen")
check(bool(re.fullmatch(r"\d+", read(".node-version").strip())), ".node-version muss eine Node-Hauptversion sein")

toolchain = tomllib.loads(read("rust-toolchain.toml")).get("toolchain", {})
check(bool(re.fullmatch(r"\d+\.\d+\.\d+", toolchain.get("channel", ""))), "rust-toolchain.toml muss eine exakte Rust-Version nennen")
check({"clippy", "rustfmt"} <= set(toolchain.get("components", [])), "rust-toolchain.toml braucht die Komponenten clippy und rustfmt")

# Der Release-Workflow baut nur, wenn alle drei Versionen übereinstimmen, und
# holt die Release-Notes aus dem passenden CHANGELOG-Abschnitt. Die Version
# landet in Tag, Dateinamen und latest.json, daher nur reines x.y.z.
tauri = json.loads(read("src-tauri/tauri.conf.json"))["version"]
cargo = tomllib.loads(read("src-tauri/Cargo.toml"))["package"]["version"]
versions = {"src-tauri/tauri.conf.json": tauri, "package.json": package["version"], "src-tauri/Cargo.toml": cargo}
check(len(set(versions.values())) == 1, f"Versionskonflikt: {versions}")
check(bool(re.fullmatch(r"\d+\.\d+\.\d+", tauri)), f"Version muss x.y.z sein, ist {tauri!r}")

# Ohne nachgezogenes Cargo.lock scheitern die --locked-Läufe in CI und Release.
lock = tomllib.loads(read("src-tauri/Cargo.lock"))
locked = [p["version"] for p in lock.get("package", []) if p.get("name") == "tippit"]
check(locked == [cargo], f"Cargo.lock nennt für tippit {locked}, Cargo.toml {cargo}: cargo check in src-tauri ausführen")

check(
    re.search(rf"^## \[{re.escape(tauri)}\]", read("CHANGELOG.md"), re.M) is not None,
    f"CHANGELOG.md hat keinen Abschnitt ## [{tauri}]",
)

if fehler:
    print("\n".join(f"Fehler: {meldung}" for meldung in fehler), file=sys.stderr)
    sys.exit(1)
print(f"Konfiguration geprüft, Version {tauri}, Rust {toolchain['channel']}.")
