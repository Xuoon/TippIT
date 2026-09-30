# TippIT

Tray-App für Windows und macOS (nur Apple Silicon), Rust + Tauri v2 + Svelte 5 (SvelteKit static). Ein Hotkey tippt die Zwischenablage als Tastatureingaben, ein zweiter öffnet die verschlüsselte Historie. Alles bleibt auf dem Gerät; die einzige Netzverbindung ist die Update-Prüfung.

## Befehle

- Frischer Clone: erst `bun install && bun run build`, sonst scheitern `cargo check/test/clippy` (`generate_context!` braucht `../build`).
- Prüfen wie die CI: in `src-tauri/` `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`; im Root `bun run check`, `bun run typecheck`, `bun test`. Formatieren: `bun run fix`.
- Laufendes TippIT vor `bun dev` beenden (Single-Instance). `bun run tauri build` signiert Updater-Artefakte mit `TAURI_SIGNING_PRIVATE_KEY` aus `.env.local`; nur der Launcher `scripts/tauri.js` reicht die Datei an Tauri weiter.
- Windows-Bundling mit „os error 5": NSIS-Toolset von Hand nach `%LOCALAPPDATA%\tauri\NSIS` legen (samt `Plugins/x86-unicode/additional/nsis_tauri_utils.dll`).
- Cloud-Container: `bash .github/scripts/setup.sh`; Rust-Prüfungen laufen unter Linux nicht (`platform/` kennt nur Windows und macOS), nur in der CI.
- Release: Version in `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` und `package.json` gleich anheben plus CHANGELOG-Abschnitt; der Push auf `main` released über `release.yml`.

## Invarianten

- **OS-APIs nur in `src-tauri/src/platform/`** (`win.rs`/`mac.rs` mit identischer API); Fachmodule bleiben plattformneutral. Im Frontend ist `src/lib/platform.ts` die einzige Plattformweiche.
- **Krypto-Crates bleiben auf ihrer Serie** (`aes-gcm 0.10`, `hkdf`/`pbkdf2`/`hmac 0.12`, `sha2 0.10`): die Nachfolger haben eine inkompatible API.
- **Formate nie ändern:** Ciphertext `nonce(12) ‖ AES-256-GCM`, AAD `version ‖ kind ‖ uuid`; der Rich-Text-Blob nutzt `db::AAD_HTML`, damit er nicht gegen den Klartext derselben Zeile tauschbar ist. Exportdatei (`storage/portable.rs`) `magic ‖ salt ‖ runden ‖ nonce ‖ AES-256-GCM` mit dem Kopf als AAD. Export/Import ist der einzige Umzugsweg, weil `key.bin` maschinengebunden ist; der Import überschreibt nie.
- **Eigene Clipboard-Writes:** nach jedem erfolgreichen Write `clipboard::read::mark_own_write` aufrufen, sonst erfasst der Monitor die eigene Kopie.
- **Fremd-HTML** wird beim Erfassen und beim Ausliefern sanitisiert (ammonia), nie roh gespeichert. Die Historie-WebView lädt nur App-URLs (`windows_util::is_app_url`).
- **Tippen:** vor der Injektion auf das Loslassen aller Modifier warten (`typing::wait_modifiers_released`). ESC wird nur während eines Tippvorgangs global registriert (`EscCancelGuard`), sonst schluckt TippIT systemweit ESC; jede Warte- und Injektionsphase prüft `typing::alive`.
- **Historie-Fenster nie zerstören, nur verstecken.** Vor dem Anzeigen das Tipp-Ziel merken (`prev_target`). Sichtbarkeit nur über `platform::window_visible`/`hide_window`, nie Tauris `is_visible`/`hide`. Lage und Größe in logischen Einheiten des Zielmonitors (`platform::place_window`); gespeichert wird nur eine Lage, die vom zuletzt selbst gesetzten Rahmen abweicht (`windows_util::PLACEMENT`).
- **Globaler Maus-Hook nur für die Mini-Palette** (`palette.rs`, Hook selbst nur in `platform/`: Windows `WH_MOUSE_LL`, macOS CGEventTap): bewusst ein Experiment trotz AV-Risiko, ab Werk aus. Tipp-Ziel ist die angeklickte App, nicht das Vordergrundfenster (der Klick wird verschluckt und vor dem Einfügen an derselben Stelle nachgestellt, `platform::click_at`; der Hook muss diesen eigenen Klick durchlassen); macht er Probleme, die Palette auf Hotkey-Auslösung umbauen.
- **`tauri-plugin-single-instance` bleibt das erste Plugin**, schwere Initialisierung gehört in `setup()`.
- **Defaults** nur in `src-tauri/defaults.json` bzw. den Code-Defaults; das Frontend holt sie über `default_settings`, nie duplizieren.
- **Löschen heißt Papierkorb:** endgültig entfernen nur `purge_entry`, `empty_trash`, das Eintragslimit (`db::prune`), das Zusammenführen beim Wiederherstellen (`db::restore_merging`, das inhaltsgleiche Duplikat) und die 30-Tage-Frist.
- **Textbausteine** sind von Limit, Frist und „Historie leeren" ausgenommen und stehen vor Angepinntem. Platzhalter wie `{datum}` löst nur `history::resolve_text` für Bausteine auf.
- **DB-Migrationen** (`storage/db.rs`): Fresh-CREATE bleibt v1, jede Erweiterung ist ein ALTER-Schritt darüber.
- **Ausgeschlossene Apps** vergleichen gegen ID und Anzeigename; eine unbekannte App (`None`) wird nie ausgeschlossen.
- **Öffnen und Einfügen:** `open_entry`/`open_link` öffnen nur http(s) (und Dateipfade), nie andere Schemes. `Inject::Paste` löst Strg/⌘+V aus; der Aufrufer legt den Inhalt vorher in die Zwischenablage. TOTP tippt den live erzeugten Code, nie das Secret.
- Datenverzeichnis fest `~/.labi/tippit/` (Windows `%USERPROFILE%`).

## Frontend

- Farben, Typo und Radien nur über `var(--…)` aus `src/lib/theme.css`; Theme-Farben in `:root` und `:root[data-theme="light"]`. Jede Route ruft in `onMount` `initTheme()` auf.
- Eintrags-Typen, Filter und Primäraktionen nur in `src/lib/entry-kinds.ts`. TOTP und Links filtert Rust (`storage/index.rs::Refine`); `is_link`/`is_totp` spiegeln `isLink`/`isTotp` mit denselben Testfällen.
- Die Historie-Liste ist virtualisiert und seitenweise geladen: `ROW_H`/`HEAD_H` in `history-list.svelte` müssen zu `--row-h` und `.group-head` passen.
- Kein `<style>`-Element in `app.html`: die CSP erlaubt Styles nur über `'unsafe-inline'`, eine Nonce bräche alle `style`-Attribute.

## macOS

- Nur `aarch64-apple-darwin`; Updater-Key `darwin-aarch64`.
- Builds sind ad hoc signiert: die Bedienungshilfen-Freigabe hängt am Binary-Hash und gilt je Build neu. Den Systemdialog löst nur eine Nutzeraktion in den Einstellungen aus (`platform::request_input_permission`); Start und Tippen prüfen nur. Aus demselben Grund kein Keychain für `key.bin`.
- Transparente Fenster brauchen das Tauri-Feature `macos-private-api` plus `app.macOSPrivateApi`.

## Windows

- Hotkeys zweigleisig: `global-shortcut` plus `GetAsyncKeyState`-Fallback für abfangende Apps (TeamViewer), dedupliziert in `hotkeys::handle`.
- WinRT-Async (OCR) mit `.join()` statt `.get()` abwarten.
- Releases signiert `release.yml` über Azure Artifact Signing, siehe [.github/SIGNING.md](.github/SIGNING.md).
