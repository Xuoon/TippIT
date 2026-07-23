# TippIT

Kleine Tray-App für Windows und macOS (Apple Silicon): tippt die Zwischenablage als echte Tastatureingaben (funktioniert auch dort, wo Einfügen blockiert ist — RDP, VMs, Passwortfelder) und bringt eine persistente, verschlüsselte, durchsuchbare Zwischenablage-Historie mit optionalem Ende-zu-Ende-verschlüsseltem Multi-Device-Sync mit.

Rust + Tauri v2 + Svelte 5.

## Features

- **STRG + Y** (macOS: **⌘Y**) — Zwischenablage tippen: trimmt Whitespace, 440-Hz-Beep, 1 s Verzögerung (Zeit zum Fokussieren), dann wird der Text als literale Tastatureingaben injiziert (Windows: `SendInput` mit `KEYEVENTF_UNICODE`, macOS: CGEvents — alle Sonderzeichen wörtlich; macOS fragt dafür einmalig die Bedienungshilfen-Berechtigung ab). Verzögerung, Modus (zeichenweise [Standard, zuverlässig auch in RDP/Citrix] / alles auf einmal), Trim und Beep sind konfigurierbar.
- **STRG + SHIFT + Y** (macOS: **⌘⇧Y**) — Historie: durchsuchbares Popup (Fuzzy-Suche beim Tippen, Filter nach Text/Bild/Links/Dateien/TOTP, Sortierung, Quellanwendung und optionale Vorschau/Details). Enter = kopieren, Strg/⌘+Enter = als Tastatur tippen, Strg/⌘+P = anpinnen, Strg/⌘+Entf = löschen, Esc = schließen, Tab = Filter wechseln.
- **Historie**: persistent (Standard 500 Einträge, konfigurierbar 100–5000), erfasst Text, Bilder (mit Thumbnails) und kopierte Dateipfade. Duplikate wandern nach oben. Pins verfallen nie.
- **Tray-Menü**: Historie, Pausieren (Icon blinkt), Sounds, Autostart (Windows: HKCU-Run-Key, macOS: LaunchAgent), Einstellungen, Beenden.
- **Verschlüsselung**: Inhalte liegen lokal als AES-256-GCM-Ciphertext in SQLite (`~/.labi/tippit/history.db`). Der Schlüssel in `key.bin` wird unter Windows per DPAPI (User-Scope) geschützt, unter macOS per Dateirechten (0600) + FileVault.
- **Sync (optional)**: E2E-verschlüsselt über ein eigenes Convex-Deployment, ohne Account. Kopplung per langem Code (`TIPPIT-XXXXX-…`, auch als QR). Der Server sieht ausschließlich Ciphertext.
- **Updates**: TippIT prüft beim Start im Hintergrund auf signierte Updates und kann sie direkt aus dem Hinweis oder den Einstellungen installieren.

## Entwicklung

Voraussetzungen: Rust und [Bun](https://bun.sh); unter Windows zusätzlich VS Build Tools mit C++-Workload (MSVC), unter macOS die Xcode Command Line Tools.

```powershell
bun install
bun dev               # Turbo-TUI: Tauri/Vite mit HMR + Convex; laufendes TippIT vorher beenden
bun run tauri build   # Release: NSIS-Setup (Windows) bzw. App + DMG (macOS) + Updater-Signatur
bun run fix           # Biome/Ultracite: Lint + Format anwenden (prüfen: bun run check)
cargo test            # Krypto-Unit-Tests (in src-tauri/)
```

## Releases

Ein Push auf `main` mit derselben erhöhten Version in `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` und `package.json` erzeugt automatisch einen GitHub-Release mit NSIS-Setup (Windows), DMG + Update-Tarball (macOS, nur Apple Silicon) und einem gemeinsamen `latest.json`; die Release-Notes kommen aus dem passenden `CHANGELOG.md`-Abschnitt. Ohne Versionssprung wird kein Release erstellt. Der Workflow erwartet den privaten Updater-Schlüssel im Repository-Secret `TAURI_SIGNING_PRIVATE_KEY`.

Das NSIS-Setup beendet eine laufende TippIT-Instanz automatisch und räumt bei Deinstallation den Autostart-Eintrag auf. Updates werden vor der Installation mit dem eingebetteten öffentlichen Schlüssel geprüft. Die macOS-Builds sind nicht notariell beglaubigt: Beim ersten Öffnen des DMG-Installs Rechtsklick → „Öffnen" (danach übernehmen die signierten In-App-Updates).

Daten & Logs: `~/.labi/tippit/` bzw. `%USERPROFILE%\.labi\tippit\` (`settings.json`, `key.bin`, `history.db`, `sync.json`, `logs/`). Der Ordner `.labi` wird unter Windows ausgeblendet.

## Sync einrichten

Für Anwender: In **Einstellungen → Sync** auf Gerät 1 **„Sync aktivieren"** klicken und den TippIT-Code kopieren/anzeigen; auf Gerät 2 den Code unter **„Mit Code beitreten"** einfügen. Die Server-URL ist in der fertigen EXE bereits vorausgefüllt (`src-tauri/defaults.json`, wird beim Build eingebettet).

Für Betreiber: Das Convex-Backend liegt in `convex/`. Einmalig `bunx convex dev` (Entwicklung) bzw. `bunx convex deploy` (Produktion) ausführen und die Deployment-URL in `src-tauri/defaults.json` eintragen, bevor die EXE gebaut wird. Abweichende URLs lassen sich pro Gerät unter Einstellungen → Sync → Erweitert setzen.

Der Kopplungscode enthält das Gruppen-Secret — wie ein Passwort behandeln und als Wiederherstellungscode notieren. **Code weg + alle Geräte weg = Daten in der Cloud sind nicht mehr entschlüsselbar.**

Standardmäßig werden Text-Einträge und Pins synchronisiert; Bilder optional (mit Größenlimit). Programm-Einstellungen bleiben geräte-lokal. Die Einstellungen zeigen unter Sync die Geräte der Gruppe (Name, Plattform, zuletzt aktiv). „Gruppe verlassen" erzeugt ein frisches lokales Secret (alte Gruppenmitglieder können künftige Daten nicht lesen), die lokale Historie bleibt.

Mobilfunk, Energiesparmodus (Windows-Energiesparmodus bzw. macOS-Stromsparmodus) und Datensparmodus sind für Hintergrund-Sync standardmäßig gesperrt. Diese Regeln und der Sync-Abstand lassen sich direkt in den Einstellungen ändern.

## Sicherheitsmodell

**Schützt gegen:**

- Auslesen der Datenbank/Backups ohne Anmeldung am Benutzerkonto (Windows: DPAPI, macOS: 0600 + FileVault; dazu AES-256-GCM pro Eintrag, AAD bindet Ciphertext an Eintrag und Typ)
- Kompromittierten/neugierigen Sync-Server: Convex speichert nur Ciphertext und grobe Metadaten (Typ, Größe, Zeitstempel, Pin-Flag); der Auth-Key liegt serverseitig nur als SHA-256-Hash
- Fremde Sync-Gruppen: Gruppen-ID ist aus 128 Bit Zufall abgeleitet und unratbar; Schreibzugriff erfordert den Auth-Key

**Schützt nicht gegen:**

- Malware, die unter deinem Benutzerkonto läuft (kann DPAPI aufrufen bzw. die Schlüsseldatei lesen und RAM auslesen — das gilt für jeden Clipboard-Manager)
- Jemanden, der den Kopplungscode erfährt
- Böswillige Gruppenmitglieder: Wer das Secret hat, ist voll vertrauenswürdig — er kann Einträge überschreiben/löschen
- Metadaten-Analyse auf dem Server (wie viele Einträge, wann, wie groß)

## Bekannte Grenzen

- **Elevated Fenster (UIPI):** Tastatur-Injektion in ein als Administrator laufendes Fenster wird von Windows still verworfen, solange TippIT nicht selbst elevated läuft.
- **SmartScreen/Defender:** Die unsignierte EXE (Clipboard + SendInput) kann heuristisch anschlagen. TippIT verwendet bewusst keine Hooks (`SetWindowsHookEx`).
- **Hotkey-Konflikte:** STRG+Y ist mancherorts „Wiederholen". Registrierungsfehler landen im Log; Hotkeys sind unter Einstellungen → Hotkeys änderbar.
- Bilder größer als das Limit (bzw. ~900 KB Ciphertext) bleiben lokal und werden nicht gesynct.
- OCR nutzt unter Windows die eingebaute Windows-Texterkennung — erkannt wird, was als Sprachpaket installiert ist (Einstellungen → Zeit und Sprache); ohne Sprachpaket meldet TippIT das beim Extrahieren.
