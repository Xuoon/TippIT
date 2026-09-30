# TippIT

Tray-App für Windows und macOS (nur Apple Silicon), Rust + Tauri v2 + Svelte 5/SvelteKit static: STRG+E (macOS: ⌘E) tippt die Zwischenablage als Tastatureingaben, STRG+SHIFT+E (⌘⇧E) öffnet die verschlüsselte Historie. Alles ist geräte-lokal; kein Backend, keine Übertragung von Inhalten; die einzige Netzverbindung ist die Update-Prüfung.

## Commands

- Laufendes TippIT vor `bun dev` beenden. `bun run tauri build` baut und signiert mit `TAURI_SIGNING_PRIVATE_KEY` aus `.env.local`; das lädt nur der Launcher `scripts/tauri.js`, ein direkter Tauri-Aufruf scheitert beim Signieren.
- Windows-Bundling mit „os error 5": NSIS-Toolset manuell nach `%LOCALAPPDATA%\tauri\NSIS` legen (inkl. `Plugins/x86-unicode/additional/nsis_tauri_utils.dll`).
- In `src-tauri/`: `cargo test`, `cargo fmt --check` und `cargo clippy --all-targets -- -D warnings` (so prüft CI); Frontend-Typcheck: `bun run typecheck`; Lint/Format: `bun run fix` (prüfen: `bun run check`).
- Messpunkte der Historie (`src/lib/perf.ts`, Öffnen/Suche) loggen im Dev-Build, im Release nur mit localStorage `tippit.perf` = `1`, in die WebView-Konsole.
- Cloud-Container: `bash .github/scripts/setup.sh` installiert Node und Bun samt Abhängigkeiten; Rust-Prüfungen gehen unter Linux nicht (`platform/` kennt nur Windows und macOS) und laufen in der CI.
- Frischer Clone: erst `bun install && bun run build`, sonst scheitern kompilierende cargo-Befehle wie `cargo check/test/clippy` (`generate_context!` verlangt `../build`)
- Release: Version in `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` und `package.json` bumpen + CHANGELOG-Abschnitt → Push auf `main` released automatisch (`.github/workflows/release.yml`, baut Windows + macOS und published ein `latest.json` mit beiden Plattform-Keys)

## Invarianten

- **Direkte native OS-APIs nur in `src-tauri/src/platform/`** (`win.rs`/`mac.rs`, identische API); Fachmodule bleiben plattformneutral. Ziel-/Dependency-`cfg` in Manifesten bzw. Modul-Wiring ist zulässig. Im Frontend ist `src/lib/platform.ts` die einzige Plattformweiche (Modifier, Hotkey-Labels, Symbol-Parität mit `platform::{win,mac}.rs::display_hotkey`).
- **Krypto-Crates bleiben auf der 0.10/0.12-Serie** (`aes-gcm 0.10`, `hkdf 0.12`, `sha2 0.10`, `pbkdf2 0.12`, `hmac 0.12`, `src-tauri/Cargo.toml`); die Nachfolgeserie hat eine inkompatible API (hybrid-array statt GenericArray).
- **Ciphertext-Format nie ändern:** `nonce(12) ‖ AES-256-GCM`, AAD = `version ‖ kind ‖ uuid` (`storage/crypto.rs`). Das AAD-Byte `kind` ist nicht immer der Eintragstyp: der Rich-Text-Blob einer Zeile nutzt `db::AAD_HTML` (3), damit er nicht gegen den Klartext-Blob derselben Zeile tauschbar ist. Keine Schlüsselrotation: `state.keys` steht ab `setup()` fest, ohne RwLock.
- **Eigene Clipboard-Writes:** Nach jedem erfolgreichen `set_text`/`set_image`/`clipboard_set_html` `clipboard::read::mark_own_write(&state)` aufrufen (`history.rs`); der Monitor überspringt genau diese Sequenznummer; ein Zähler-Ansatz leckt bei fehlgeschlagenen Writes.
- **Fremd-HTML wird beim Erfassen sanitisiert** (`clipboard/html.rs`, ammonia) und beim Ausliefern an die WebView erneut (`history::entry_html`). Roh gespeichert wird es nie; die Vorschau rendert es per `{@html}` in einer WebView mit Tauri-Command-Zugriff, die nur App-URLs lädt (`windows_util::is_app_url`). Frontend-seitig erzeugtes Vorschau-HTML (`lib/preview.ts`, `lib/highlight.ts`) escaped den Quelltext selbst und setzt nur eigene Tags; Farbwerte für `style` kommen aus einer Regex ohne `;`/`:`.
- **Vor der Injektion auf physisches Loslassen aller Modifier warten** (`typing.rs::wait_modifiers_released`), sonst feuert der getippte Text als Shortcuts im Zielfenster. Abbruch ist fest ESC und wird nur für die Dauer eines Vorgangs global registriert (`typing::EscCancelGuard`), nie dauerhaft, sonst schluckt TippIT systemweit jede ESC-Taste. Jede Warte- oder Injektionsphase muss `typing::alive(app, generation)` prüfen (Fokus-Restore, Beep, Chunk-Schleifen), sonst läuft ESC dort ins Leere.
- **Historie-Fenster nie zerstören, nur verstecken** (`windows_util.rs`): `CloseRequested` → `prevent_close()` + hide. Beim Öffnen wird zuerst das Vordergrund-Ziel als Tipp-Ziel gemerkt (`prev_target`), dann das Fenster aktiviert. Sichtbarkeit immer über `platform::window_visible`/`hide_window`, nie Tauris `is_visible`/`hide`: ein ohne Aktivierung gezeigtes Fenster fehlt in Tauris Visible-Zustand, `hide()` wäre dann ein No-op. Fokusverlust steuert `history.close_on_blur` (`windows_util::on_history_blur`): versteckt wird nur, wenn eine fremde App vorn ist, nie bei eigenen Fenstern/Dialogen, unbekannter App (`None`) oder laufendem Tippvorgang. Größe und Lage rechnen in logischen Einheiten des Zielmonitors (`platform::Frame`, `place_window`), nie mit dem Faktor des Fensters; `Moved` speichert `history.window_position` nur, wenn der Rahmen vom zuletzt selbst gesetzten (`windows_util::PLACEMENT`) abweicht.
- **`tauri-plugin-single-instance` muss das erste Plugin bleiben** und schwere Initialisierung gehört in `setup()` (`lib.rs`), damit Zweitstarts sofort abbrechen.
- **Auslieferungs-Defaults** stehen in `src-tauri/defaults.json` und werden per `include_str!` in die EXE eingebettet (`storage/settings.rs::shipped_defaults`); das Frontend liest sie über den Command `default_settings`; Defaults nie im Frontend duplizieren.
- **Löschen heißt Papierkorb, nicht weg:** `delete_entry`/`clear_history` setzen `trashed_at` und lassen den Ciphertext stehen (`db::trash`), damit `restore_entry` etwas zurückholen kann. Endgültig entfernen nur `purge_entry`, `empty_trash`, `db::prune` (Eintragslimit; bewusst ohne Papierkorb, sonst wäre er eine zweite unbegrenzte Historie) und die 30-Tage-Frist in `monitor::run_retention`. Aufbewahrungsfristen laufen einmal pro Programmstart und beim Ändern der Frist, nicht bei jeder Kopie.
- **Textbausteine (`snippet = 1`) sind von Limit, Aufbewahrungsfrist und „Historie löschen" ausgenommen** und stehen im Index vor Angepinntem. Platzhalter (`{datum}` …) löst nur `history::resolve_text` und nur für Bausteine auf; in einer erfassten Kopie ist `{datum}` gewollter Text.
- **DB-Migrationen** (`storage/db.rs`): fresh-CREATE bleibt v1-Shape, jede Erweiterung ist ein ALTER-Schritt darüber; gewachsene und frische Datenbanken nehmen denselben Pfad.
- **Export/Import (`storage/portable.rs`) ist der einzige Umzugsweg**, weil `key.bin` maschinengebunden ist. Dateiformat: `magic(16) ‖ salt(16) ‖ runden(u32 BE) ‖ nonce(12) ‖ AES-256-GCM`, Kopf als AAD (sonst ließen sich Salt/Rundenzahl unbemerkt drehen). Der Import verschlüsselt mit dem Geräteschlüssel neu und überspringt vorhandene uuids/Hashes; er überschreibt nie.
- **Ausschlussliste der Quell-Apps** (`history.excluded_apps`) vergleicht gegen ID **und** Anzeigename. Eine unbekannte Vordergrund-App (`None`) wird nie ausgeschlossen, sonst ließe ein Erkennungsfehler die Historie still leerlaufen.
- **Fenster-Chrome von Historie und Update-Hinweis je Plattform** (`platform::TRANSPARENT_WINDOW`, Frontend `applyWindowChrome` → `html[data-chrome]`): macOS transparent mit CSS-Radius, das kompiliert nur mit Tauri-Feature `macos-private-api` + `app.macOSPrivateApi` in `src-tauri/tauri.conf.json`; Windows opak mit DWM-Ecken und -Schatten (ein transparentes Fenster hätte einen eckigen Schatten). Settings bleibt dekoriert und opak, `app.html` ist der opake Fallback.
- **TOTP:** Kopieren/Tippen nutzt den clientseitig live erzeugten Code (`totp.ts`), nie das Secret. `open_entry` öffnet nur http(s) und `KIND_FILES`-Pfade, `open_link` nur http(s), nie beliebige Schemes. `typing::Inject::Paste` (Enter/Doppelklick in der Historie, auch für Bilder) löst STRG+V/⌘V aus; der Aufrufer muss den Inhalt vorher in die Zwischenablage legen, sonst fügt es den alten ein.
- **Bilder:** Die Liste nutzt `entry_thumb` (≤256px), die Detail-Vorschau `entry_image` (Vollbild, lazy nur für den ausgewählten Eintrag, Blob kann mehrere MB groß sein). Nie vertauschen: aus dem Thumbnail wird die Vorschau unscharf, aus Vollbildern frisst die Liste Speicher.
- Datenverzeichnis ist fest `~/.labi/tippit/` (Windows: `%USERPROFILE%`, `storage/paths.rs`), nicht APPDATA/Application Support; kein Legacy-Fallback auf `.labit`.

## Frontend

- Farben/Typo/Radien nur über `var(--…)` aus `src/lib/theme.css`, nie rohe Hex-Werte im Komponenten-CSS. Strukturelle Tokens (Radien, Density, Typo, Timing) nur in `:root`; Theme-Farben/Shadows in beiden Blöcken (`:root` + `:root[data-theme="light"]`).
- Jede Route ruft in `onMount` `initTheme()` aus `src/lib/theme.ts` auf und gibt dessen Cleanup zurück.
- Neue Eintrags-Typen/Filter/Primäraktionen (`primaryAction`) der Historie nur in `src/lib/entry-kinds.ts` registrieren. TOTP und Links sind Text-Verfeinerungen, keine Backend-kinds; gefiltert wird in Rust (`storage/index.rs::Refine`), `is_link`/`is_totp` spiegeln `isLink`/`isTotp` mit denselben Testfällen.
- **Die Historie-Liste ist virtualisiert** (`src/lib/components/history-list.svelte`): gerendert wird nur das Sichtfenster, und `search_history` liefert Sortierung, Filter und Seiten (Nachladen beim Scrollen). Das funktioniert nur mit festen Zeilenhöhen; `ROW_H`/`HEAD_H` im Script müssen zu `--row-h` bzw. der `.group-head`-Regel passen, sonst wandert die Auswahl aus dem Bild.
- CSP (`src-tauri/tauri.conf.json`) erlaubt Styles nur über `'unsafe-inline'`: kein `<style>`-Element in `app.html`, sonst hängt Tauri eine Nonce an und alle `style`-Attribute brechen.

## macOS

- Nur Apple Silicon (`aarch64-apple-darwin`), keine Intel- oder Universal-Builds; der Updater-Key ist `darwin-aarch64`.
- Builds sind nur ad-hoc signiert: Die Bedienungshilfen-Freigabe hängt am Binary-Hash und gilt je Build und Update neu. Ohne sie verwirft macOS CGEvents still, deshalb bleibt der Guard in `typing.rs`. Den Systemdialog löst nur eine Nutzeraktion im Abschnitt „Berechtigungen“ der Einstellungen aus (`platform::request_input_permission`), Start und Tippen prüfen nur (`input_permission_granted`).
- Bewusst kein Keychain für `key.bin`: Die ACL bände bei Ad-hoc-Signatur an den Binary-Hash, nach jedem Update käme ein Passwort-Prompt.

## Windows

- Globale Hotkeys bleiben zweigleisig: `global-shortcut`/`WM_HOTKEY` plus `GetAsyncKeyState`-Fallback für abfangende Vordergrund-Apps wie TeamViewer; beide laufen über `hotkeys::handle` und werden dort dedupliziert.
- OCR (WinRT `Windows.Media.Ocr`) wartet mit `.join()`, nicht `.get()`.
- Releases signiert `release.yml` über Azure Artifact Signing, lokale und Testbuilds bleiben unsigniert; Einrichtung und Störungen in [.github/SIGNING.md](.github/SIGNING.md).
