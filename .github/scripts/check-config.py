"""Konfigurationen parsen und die Versionsverträge von Toolchain und Release prüfen."""

import json
from pathlib import Path
import re
import tomllib

root = Path(__file__).resolve().parents[2]
for path in [*(root / "src-tauri").glob("*.json"), *(root / "src-tauri/capabilities").glob("*.json")]:
    json.loads(path.read_text(encoding="utf-8"))

package = json.loads((root / "package.json").read_text(encoding="utf-8"))
assert re.fullmatch(r"bun@\d+\.\d+\.\d+", package["packageManager"]), "packageManager muss eine exakte Bun-Version nennen"
assert re.fullmatch(r"\d+", (root / ".node-version").read_text().strip()), ".node-version muss eine Node-Hauptversion sein"

# Der Release-Workflow baut nur, wenn alle drei Versionen übereinstimmen, und
# holt die Release-Notes aus dem passenden CHANGELOG-Abschnitt.
tauri = json.loads((root / "src-tauri/tauri.conf.json").read_text(encoding="utf-8"))["version"]
cargo = tomllib.loads((root / "src-tauri/Cargo.toml").read_text(encoding="utf-8"))["package"]["version"]
versions = {"tauri.conf.json": tauri, "package.json": package["version"], "Cargo.toml": cargo}
assert len(set(versions.values())) == 1, f"Versionskonflikt: {versions}"
changelog = (root / "CHANGELOG.md").read_text(encoding="utf-8")
assert re.search(rf"^## \[{re.escape(tauri)}\]", changelog, re.M), f"CHANGELOG.md hat keinen Abschnitt ## [{tauri}]"
print(f"Konfiguration geprüft, Version {tauri}.")
