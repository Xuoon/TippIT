---
paths:
  - "src-tauri/src/platform/win.rs"
  - "src-tauri/src/hotkeys.rs"
  - "src-tauri/Cargo.toml"
---

# Windows-Spezifika

- Globale Hotkeys bleiben zweigleisig: `global-shortcut`/`WM_HOTKEY` plus physischer `GetAsyncKeyState`-Fallback für abfangende Vordergrund-Apps wie TeamViewer; beide Pfade laufen über `hotkeys::handle` und werden dort dedupliziert.
- OCR (WinRT `Windows.Media.Ocr`): Async blockierend via `.join()` — `windows 0.62` zieht `windows-future 0.3`, dort gibt es `.get()` nicht mehr. Erkennung folgt den installierten Windows-Sprachpaketen.
