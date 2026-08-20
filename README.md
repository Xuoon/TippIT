# TippIT

Kleine Tray-App für Windows und macOS (Apple Silicon): tippt die Zwischenablage als echte Tastatureingaben (funktioniert auch dort, wo Einfügen blockiert ist — RDP, VMs, Passwortfelder) und bringt eine persistente, verschlüsselte, durchsuchbare Zwischenablage-Historie mit. Alles bleibt auf dem Gerät — kein Konto, kein Server, keine Übertragung.

Rust + Tauri v2 + Svelte 5.

## Features

- **STRG + E** (macOS: **⌘E**) — Zwischenablage tippen: trimmt Whitespace, 440-Hz-Beep, 1 s Verzögerung (Zeit zum Fokussieren), dann wird der Text als literale Tastatureingaben injiziert (Windows: `SendInput` mit `KEYEVENTF_UNICODE`, macOS: CGEvents — alle Sonderzeichen wörtlich; macOS fragt dafür einmalig die Bedienungshilfen-Berechtigung ab). Verzögerung, Modus (zeichenweise [Standard, zuverlässig auch in RDP/Citrix] / alles auf einmal), Trim und Beep sind konfigurierbar.
- **STRG + SHIFT + E** (macOS: **⌘⇧E**) — Historie: durchsuchbares Popup (Fuzzy-Suche beim Tippen, Filter nach Bausteinen/Text/Bild/Links/Dateien/TOTP, Sortierung, Quellanwendung und optionale Vorschau/Details). Enter = tippen, Doppelklick = einfügen (mit Strg/⌘ stattdessen zeichenweise tippen), ⇧+Enter = Link/Datei öffnen bzw. Text extrahieren, Strg/⌘+1…9 = n-ten Eintrag tippen, Strg/⌘+P = anpinnen, Strg/⌘+B = als Textbaustein merken, Strg/⌘+Entf = in den Papierkorb, `?` = Tastenkürzel, Esc = schließen, Tab = Filter wechseln.
- **Historie**: persistent (Standard 500 Einträge, konfigurierbar 100–5000), erfasst Text, Bilder (mit Thumbnails) und kopierte Dateipfade. Duplikate wandern nach oben, Pins verfallen nie. Die Liste rendert nur das Sichtfenster und bleibt damit auch bei tausenden Einträgen flüssig. Optional lassen sich Einträge nach einer Frist automatisch aufräumen und einzelne Quellprogramme (Passwortmanager) komplett ausschließen.
- **Textbausteine**: dauerhafte Einträge mit den Platzhaltern `{datum}`, `{uhrzeit}`, `{datumzeit}`; von Limit, Aufbewahrungsfrist und „Historie löschen" ausgenommen.
- **Papierkorb**: Gelöschtes bleibt 30 Tage wiederherstellbar.
- **Vorschau**: erkannter Code wird eingefärbt (JSON, JS/TS, Rust, Python, SQL, Shell, CSS, HTML — Sprache umschaltbar), http(s)-Adressen sind anklickbar, Farbwerte bekommen eine Farbprobe, Suchtreffer werden markiert. Formatierter Text (HTML aus der Zwischenablage) wird sanitisiert mitgespeichert und beim Einfügen wieder formatiert übergeben — getippt wird immer Klartext.
- **Tray-Menü**: Historie, Pausieren (Icon blinkt), Sounds, Autostart (Windows: HKCU-Run-Key, macOS: LaunchAgent), Einstellungen, Beenden.
- **Verschlüsselung**: Inhalte liegen lokal als AES-256-GCM-Ciphertext in SQLite (`~/.labi/tippit/history.db`). Der Schlüssel in `key.bin` wird unter Windows per DPAPI (User-Scope) geschützt, unter macOS per Dateirechten (0600) + FileVault.
- **Sicherung**: Export in eine passwortgeschützte Datei (PBKDF2-SHA256 + AES-256-GCM) und Import, der mit der vorhandenen Historie zusammenführt — der Weg auf einen neuen Rechner.
- **Updates**: TippIT prüft beim Start im Hintergrund auf signierte Updates und kann sie direkt aus dem Hinweis oder den Einstellungen installieren.

## Entwicklung

Voraussetzungen: Rust und [Bun](https://bun.sh); unter Windows zusätzlich VS Build Tools mit C++-Workload (MSVC), unter macOS die Xcode Command Line Tools.

```powershell
bun install
bun dev               # Tauri/Vite mit HMR; laufendes TippIT vorher beenden
bun run tauri build   # Release: NSIS-Setup (Windows) bzw. App + DMG (macOS) + Updater-Signatur
bun run fix           # Biome/Ultracite: Lint + Format anwenden (prüfen: bun run check)
cargo test            # Unit-Tests (in src-tauri/)
```

## Releases

Ein Push auf `main` mit derselben erhöhten Version in `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` und `package.json` erzeugt automatisch einen GitHub-Release mit NSIS-Setup (Windows), DMG + Update-Tarball (macOS, nur Apple Silicon) und einem gemeinsamen `latest.json`; die Release-Notes kommen aus dem passenden `CHANGELOG.md`-Abschnitt. Ohne Versionssprung wird kein Release erstellt. Der Workflow erwartet den privaten Updater-Schlüssel im Repository-Secret `TAURI_SIGNING_PRIVATE_KEY`.

Das NSIS-Setup beendet eine laufende TippIT-Instanz automatisch und räumt bei Deinstallation den Autostart-Eintrag auf. Updates werden vor der Installation mit dem eingebetteten öffentlichen Schlüssel geprüft. Die macOS-Builds sind nicht notariell beglaubigt: Beim ersten Öffnen des DMG-Installs Rechtsklick → „Öffnen" (danach übernehmen die signierten In-App-Updates).

Daten & Logs: `~/.labi/tippit/` bzw. `%USERPROFILE%\.labi\tippit\` (`settings.json`, `key.bin`, `history.db`, `app-icons/`, `logs/`). Der Ordner `.labi` wird unter Windows ausgeblendet.

## Umzug auf einen anderen Rechner

`key.bin` ist an Benutzer und Maschine gebunden (Windows: DPAPI) — ein kopiertes Datenverzeichnis lässt sich anderswo nicht entschlüsseln. Der Weg führt über **Einstellungen → Daten**: dort mit einem selbst gewählten Passwort exportieren, die `.tippit`-Datei übertragen und auf dem Zielrechner mit demselben Passwort importieren. Der Import führt zusammen, statt zu überschreiben; bereits vorhandene Einträge werden übersprungen.

Das Passwort schützt die Datei allein — **Passwort weg = Sicherung nicht mehr lesbar.**

## Sicherheitsmodell

**Schützt gegen:**

- Auslesen der Datenbank/Backups ohne Anmeldung am Benutzerkonto (Windows: DPAPI, macOS: 0600 + FileVault; dazu AES-256-GCM pro Eintrag, AAD bindet Ciphertext an Eintrag und Typ)
- Mitlesen unterwegs: TippIT überträgt keine Inhalte. Die einzige Netzverbindung ist die Update-Prüfung gegen GitHub
- Weitergabe einer Sicherungsdatei: Ohne das Passwort ist sie nicht zu entschlüsseln (PBKDF2-SHA256, 600 000 Runden; Salt und Rundenzahl sind über die AAD gegen Manipulation gebunden)
- Schadhaftes HTML aus der Zwischenablage: Formatierter Inhalt wird schon beim Erfassen sanitisiert (`ammonia`) und beim Anzeigen erneut — Skripte, Event-Handler und nachladende CSS-Eigenschaften erreichen die Oberfläche nicht

**Schützt nicht gegen:**

- Malware, die unter deinem Benutzerkonto läuft (kann DPAPI aufrufen bzw. die Schlüsseldatei lesen und RAM auslesen — das gilt für jeden Clipboard-Manager)
- Jemanden, der das Export-Passwort erfährt
- Physischen Zugriff auf ein entsperrtes, angemeldetes Gerät

## Bekannte Grenzen

- **Elevated Fenster (UIPI):** Tastatur-Injektion in ein als Administrator laufendes Fenster wird von Windows still verworfen, solange TippIT nicht selbst elevated läuft.
- **SmartScreen/Defender:** Die unsignierte EXE (Clipboard + SendInput) kann heuristisch anschlagen. TippIT verwendet bewusst keine Hooks (`SetWindowsHookEx`).
- **Hotkey-Konflikte:** STRG+E ist in manchen Programmen bereits belegt. Registrierungsfehler landen im Log; Hotkeys sind unter Einstellungen → Hotkeys änderbar.
- **Spracherkennung im Code-Highlighting** rät anhand des Inhalts — ein Schnipsel hat keine Dateiendung. Bei kurzen Fragmenten liegt sie öfter daneben; die Sprache lässt sich im Vorschaubereich umstellen.
- OCR nutzt unter Windows die eingebaute Windows-Texterkennung — erkannt wird, was als Sprachpaket installiert ist (Einstellungen → Zeit und Sprache); ohne Sprachpaket meldet TippIT das beim Extrahieren.
