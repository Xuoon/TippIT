# TippIT

Windows-Tray-App (Rust + Tauri v2 + Svelte 5/SvelteKit static): STRG+E tippt die Zwischenablage als Tastatureingaben, STRG+SHIFT+E öffnet die verschlüsselte Historie; optionaler E2E-verschlüsselter Sync über Convex (`convex/`).

## Commands

- `bun run tauri dev` / `bun run tauri build` (NSIS + MSI unter `src-tauri/target/release/bundle/`)
- `cargo test` + `cargo check` in `src-tauri/`; Frontend-Typcheck: `bun run typecheck`; Lint/Format: `bun run fix` (prüfen: `bun run check`; Biome, Ultracite-Preset gevendort in `.ultracite/` — bun kann das ultracite-Paket hier nicht installieren, s. `biome.jsonc`)
- Frischer Clone: erst `bun install && bun run build`, sonst scheitert jeder cargo-Befehl (`generate_context!` verlangt `../build`)
- Release: Version in `tauri.conf.json` bumpen + CHANGELOG-Abschnitt → Push auf `main` released automatisch (`.github/workflows/release.yml`)
- NSIS-Bundling mit „os error 5": Toolset manuell nach `%LOCALAPPDATA%\tauri\NSIS` legen (inkl. `Plugins/x86-unicode/additional/nsis_tauri_utils.dll`)

## Invarianten

- **Krypto-Crates bleiben auf der 0.10-Serie** (`aes-gcm 0.10`, `hkdf 0.12`, `sha2 0.10`, `src-tauri/Cargo.toml`) — die 0.11-Serie hat eine inkompatible API (hybrid-array statt GenericArray).
- **Ciphertext-Format nie ändern:** `nonce(12) ‖ AES-256-GCM`, AAD = `version ‖ kind ‖ uuid` (`storage/crypto.rs`). Derselbe Blob dient lokal at-rest UND als Sync-Payload; Änderungen machen bestehende DBs/Cloud-Daten unlesbar.
- **Eigene Clipboard-Writes:** NACH jedem erfolgreichen `set_text`/`set_image` `clipboard::read::mark_own_write(&state)` aufrufen (`history.rs`, `sync/pairing.rs`) — der Monitor überspringt genau diese Sequenznummer; ein Zähler-Ansatz leckt bei fehlgeschlagenen Writes.
- **Vor `SendInput` auf physisches Loslassen von STRG/SHIFT/ALT/WIN warten** (`typing.rs::wait_modifiers_released`) — sonst feuert der getippte Text als Shortcuts im Zielfenster.
- **Historie-Fenster nie zerstören, nur verstecken** (`windows_util.rs`): CloseRequested → `prevent_close()` + hide (WebView2-Neuaufbau ~300–800 ms). Es öffnet per `SW_SHOWNOACTIVATE` ohne Fokusklau; das Rust-Event `history-shown` ersetzt dem Frontend den window-focus-Trigger. Kein Hide-on-blur — Schließen nur via X/Esc/Hotkey.
- **`tauri-plugin-single-instance` muss das erste Plugin bleiben** und schwere Initialisierung gehört in `setup()` (`lib.rs`), damit Zweitstarts sofort abbrechen.
- **Auslieferungs-Defaults** (z. B. Convex-URL) stehen in `src-tauri/defaults.json` und werden per `include_str!` in die EXE eingebettet (`storage/settings.rs::shipped_defaults`) — vor Release-Builds pflegen.
- **Sync-Limit synchron halten:** `MAX_INLINE_CIPHER` (900 KB) existiert doppelt in `convex/sync.ts` und `src-tauri/src/sync/mod.rs`.
- **LWW-Konflikte** entscheiden über das Tupel `(lamport, deviceId)` (`sync/protocol.rs::is_newer`); der **Pull-Cursor ist die serverseitige `seq`** (`convex/sync.ts`), NIE der Client-Lamport (Gleichstände/verspätete Pushes würden Einträge dauerhaft verlieren). Convex-Funktionen autorisieren über `sha256(authKey)`-Vergleich — neue Funktionen immer zuerst `requireGroup()` aufrufen lassen (`convex/lib.ts`).
- Datenverzeichnis ist fest `%USERPROFILE%\.labit\tippit\` (`storage/paths.rs`), nicht APPDATA.
