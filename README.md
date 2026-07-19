# TippIT

Kleine Windows-Tray-App: tippt die Zwischenablage als echte Tastatureingaben (funktioniert auch dort, wo Einfügen blockiert ist — RDP, VMs, Passwortfelder) und bringt eine persistente, verschlüsselte, durchsuchbare Zwischenablage-Historie mit optionalem Ende-zu-Ende-verschlüsseltem Multi-Device-Sync mit.

Rust + Tauri v2 + Svelte 5.

## Features

- **STRG + E** — Zwischenablage tippen: trimmt Whitespace, 440-Hz-Beep, 1 s Verzögerung (Zeit zum Fokussieren), dann wird der Text als literale Tastatureingaben injiziert (`SendInput` mit `KEYEVENTF_UNICODE`, alle Sonderzeichen wörtlich). Verzögerung, Modus (zeichenweise [Standard, zuverlässig auch in RDP/Citrix] / alles auf einmal), Trim und Beep sind konfigurierbar.
- **STRG + SHIFT + E** — Historie: durchsuchbares Popup (Fuzzy-Suche beim Tippen, Filter nach Text/Bild/Dateien, komplett tastaturbedienbar). Enter = kopieren, Strg+Enter = als Tastatur tippen, Strg+P = anpinnen, Strg+Entf = löschen, Esc = schließen, Tab = Filter wechseln.
- **Historie**: persistent (Standard 500 Einträge, konfigurierbar 100–5000), erfasst Text, Bilder (mit Thumbnails) und kopierte Dateipfade. Duplikate wandern nach oben. Pins verfallen nie.
- **Tray-Menü**: Historie, Pausieren (Icon blinkt), Sounds, Autostart (HKCU-Run-Key), Einstellungen, Beenden.
- **Verschlüsselung**: Inhalte liegen lokal als AES-256-GCM-Ciphertext in SQLite (`%USERPROFILE%\.labit\tippit\history.db`). Der Schlüssel wird per Windows-DPAPI (User-Scope) in `key.bin` geschützt.
- **Sync (optional)**: E2E-verschlüsselt über ein eigenes Convex-Deployment, ohne Account. Kopplung per langem Code (`TIPPIT-XXXXX-…`, auch als QR). Der Server sieht ausschließlich Ciphertext.

## Entwicklung

Voraussetzungen: Rust (MSVC), [Bun](https://bun.sh), VS Build Tools mit C++-Workload.

```powershell
bun install
bun run tauri dev     # Entwicklung
bun run tauri build   # Release: NSIS-Setup + MSI (src-tauri/target/release/bundle/)
bun run fix           # Biome/Ultracite: Lint + Format anwenden (prüfen: bun run check)
cargo test            # Krypto-Unit-Tests (in src-tauri/)
```

## Releases

Ein Push auf `main` mit erhöhter Version in `src-tauri/tauri.conf.json` erzeugt automatisch einen GitHub-Release mit `TippIT_v<version>_windows.exe` (NSIS-Setup) und `TippIT_v<version>_windows.msi`; die Release-Notes kommen aus dem passenden `CHANGELOG.md`-Abschnitt. Ohne Versionssprung wird kein Release erstellt.

Für Updates ist das **EXE-Setup empfohlen**: Es beendet eine laufende TippIT-Instanz automatisch und räumt bei Deinstallation den Autostart-Eintrag auf. Das MSI (per-machine, z. B. für Softwareverteilung) kann das nicht — vor einem MSI-Update TippIT beenden und nicht mit einer EXE-Installation mischen. Nutzerdaten bleiben bei Updates in beiden Fällen erhalten.

Daten & Logs: `%USERPROFILE%\.labit\tippit\` (`settings.json`, `key.bin`, `history.db`, `sync.json`, `logs\`).

## Sync einrichten

Für Anwender: In **Einstellungen → Sync** auf Gerät 1 **„Sync aktivieren"** klicken und den TippIT-Code kopieren/anzeigen; auf Gerät 2 den Code unter **„Mit Code beitreten"** einfügen. Die Server-URL ist in der fertigen EXE bereits vorausgefüllt (`src-tauri/defaults.json`, wird beim Build eingebettet).

Für Betreiber: Das Convex-Backend liegt in `convex/`. Einmalig `npx convex dev` (Entwicklung) bzw. `npx convex deploy` (Produktion) ausführen und die Deployment-URL in `src-tauri/defaults.json` eintragen, bevor die EXE gebaut wird. Abweichende URLs lassen sich pro Gerät unter Einstellungen → Sync → Erweitert setzen.

Der Kopplungscode enthält das Gruppen-Secret — wie ein Passwort behandeln und als Wiederherstellungscode notieren. **Code weg + alle Geräte weg = Daten in der Cloud sind nicht mehr entschlüsselbar.**

Standardmäßig werden Text-Einträge, Pins und Einstellungen synchronisiert; Bilder optional (mit Größenlimit). „Gruppe verlassen" erzeugt ein frisches lokales Secret (alte Gruppenmitglieder können künftige Daten nicht lesen), die lokale Historie bleibt.

## Sicherheitsmodell

**Schützt gegen:**

- Auslesen der Datenbank/Backups ohne Windows-Anmeldung (DPAPI + AES-256-GCM pro Eintrag, AAD bindet Ciphertext an Eintrag und Typ)
- Kompromittierten/neugierigen Sync-Server: Convex speichert nur Ciphertext und grobe Metadaten (Typ, Größe, Zeitstempel, Pin-Flag); der Auth-Key liegt serverseitig nur als SHA-256-Hash
- Fremde Sync-Gruppen: Gruppen-ID ist aus 128 Bit Zufall abgeleitet und unratbar; Schreibzugriff erfordert den Auth-Key

**Schützt nicht gegen:**

- Malware, die unter deinem Windows-Benutzer läuft (kann DPAPI aufrufen und RAM lesen — das gilt für jeden Clipboard-Manager)
- Jemanden, der den Kopplungscode erfährt
- Böswillige Gruppenmitglieder: Wer das Secret hat, ist voll vertrauenswürdig — er kann Einträge überschreiben/löschen und (bei aktiviertem Settings-Sync) Einstellungen aller Geräte ändern
- Metadaten-Analyse auf dem Server (wie viele Einträge, wann, wie groß)

## Bekannte Grenzen

- **Elevated Fenster (UIPI):** Tastatur-Injektion in ein als Administrator laufendes Fenster wird von Windows still verworfen, solange TippIT nicht selbst elevated läuft.
- **SmartScreen/Defender:** Die unsignierte EXE (Clipboard + SendInput) kann heuristisch anschlagen. TippIT verwendet bewusst keine Hooks (`SetWindowsHookEx`).
- **Hotkey-Konflikte:** STRG+E nutzen auch Browser/Office. Registrierungsfehler landen im Log; Hotkeys sind unter Einstellungen → Hotkeys änderbar.
- Bilder größer als das Limit (bzw. ~900 KB Ciphertext) bleiben lokal und werden nicht gesynct.
