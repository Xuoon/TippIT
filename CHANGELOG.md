# Changelog

Alle nennenswerten Änderungen an TippIT. Format angelehnt an [Keep a Changelog](https://keepachangelog.com/de/1.1.0/), Versionierung nach [SemVer](https://semver.org/lang/de/).

## [0.2.3] – 2026-07-19

### Behoben

- Schlüsselrotation (Beitritt/Austritt/neuer Code) konnte bei einem Fehler nach der Umschlüsselung die gesamte Historie unlesbar machen: jetzt Zwei-Phasen-Rotation über `key.bin.new` mit automatischer Recovery beim nächsten Start (Probe-Decrypt entscheidet, welcher Schlüssel zur DB passt)
- „Als Tastatur tippen" aus der Historie tippte nach fixer Wartezeit blind — jetzt wird verifiziert, dass das Zielfenster wirklich im Vordergrund ist; sonst Abbruch mit Fehlerton statt Fehlinjektion ins falsche Fenster
- Das Geräte-Limit „max. Einträge" erzeugte Sync-Tombstones und stutzte damit die Historie aller gekoppelten Geräte auf das kleinste Limit — lokale Verdrängung löscht jetzt nur noch lokal
- Nicht dekodierbare Sync-Einträge wurden still übersprungen, während der Pull-Cursor darüber hinweglief (Eintrag dauerhaft verloren, z. B. bei Format-Drift zwischen App-Versionen) — jetzt bricht der Pull ab und versucht es erneut
- Remote-Settings konnten eine eigene, noch nicht gepushte Einstellungsänderung überschreiben (Race mit dem Push-Debounce)
- Texte mit reinen `\r`-Zeilenumbrüchen (Alt-Mac, manche Excel-Exporte) wurden als eine zusammengeklebte Zeile getippt
- Historie-Fenster: veraltete Suchantworten konnten neuere überschreiben; fehlgeschlagene Thumbnail-Ladevorgänge blockierten das Thumbnail dauerhaft; Pin/Löschen per Tastatur ohne Fehlerbehandlung
- Einstellungen: das Echo des eigenen Speicherns setzte gerade getippte Feld-Eingaben (z. B. Server-URL) zurück; „Code anzeigen" zeigte Fehler nicht an

### Entfernt

- Wirkungsloses (nie ausgewertetes, nicht in der UI angebotenes) Setting `sync_pins`

## [0.2.2] – 2026-07-17

### Hinzugefügt

- MSI-Installer zusätzlich zum NSIS-Setup
- Automatische GitHub-Releases bei Push auf `main` (Assets `TippIT_v<version>_windows.exe`/`.msi`, Release-Notes aus dem Changelog)

### Geändert

- Zeichenweises Tippen ist jetzt der Standard-Modus (funktioniert zuverlässig auch in RDP/Citrix); „Alles auf einmal" bleibt wählbar
- Toolchain: Bun statt pnpm als Paketmanager; Biome mit Ultracite-Preset statt Prettier als Formatter/Linter

## [0.2.1] – 2026-07-17

### Behoben

- Historie-Fenster ließ sich nach dem fokuslosen Öffnen nicht mehr schließen (✕/Hotkey wirkungslos): Sichtbarkeit läuft jetzt durchgehend über Win32 (`ShowWindow`/`IsWindowVisible`) statt über Tauris internen Zustand
- Update-Installation über eine laufende TippIT-Instanz schlug fehl bzw. verlangte manuelles Deinstallieren: Installer-Hooks beenden die App vor Installation und Deinstallation automatisch
- Deinstallation räumt jetzt den Autostart-Registry-Eintrag mit auf (Nutzerdaten unter `%USERPROFILE%\.labit\tippit` bleiben erhalten)

## [0.2.0] – 2026-07-17

### Hinzugefügt

- Vorschau-Panel in der Historie: mehrzeiliger Text-Viewer (markierbar) bzw. vergrößerte Bildvorschau für den ausgewählten Eintrag
- Schließen-Button (✕) im Historie-Fenster
- Sync-Tab: „Code kopieren", „Code + QR anzeigen" und „Neuen Code erstellen" (mit Bestätigung; trennt ggf. die aktive Gruppe und verschlüsselt die Historie um)
- In die EXE eingebettete Auslieferungs-Standardwerte (`src-tauri/defaults.json`), u. a. vorbefüllte Sync-Server-URL
- Branding: Banner „TippIT · Sven Labitzki" in den Einstellungen, Tray-Tooltip „TippIT / Sven Labitzki"

### Geändert

- Historie öffnet ohne Fokusklau (`SW_SHOWNOACTIVATE`) — das zuvor aktive Fenster (z. B. Windows-Suche) bleibt offen und fokussiert
- Historie bleibt offen, bis sie aktiv geschlossen wird (✕, Esc oder Hotkey); kein automatisches Verstecken bei Fokusverlust mehr
- Server-URL in den Sync-Einstellungen unter „Erweitert" verschoben; Entwickler-Hinweise aus der UI entfernt
- NSIS-Installer als Update-Installation konfiguriert (`installMode: currentUser`, Publisher); Drüber-Installieren ersetzt die alte Version, Daten bleiben erhalten
- Alte AutoIt-/Python-Altbestände aus dem Repository entfernt

## [0.1.0] – 2026-07-16

Kompletter Neubau der AutoIt-App als Windows-Anwendung mit Rust + Tauri v2 + Svelte 5.

### Hinzugefügt

- STRG+E: Zwischenablage als literale Tastatureingaben tippen (SendInput/`KEYEVENTF_UNICODE`, Bulk- oder Zeichenweise-Modus, konfigurierbare Verzögerung, Modifier-Release-Wait)
- STRG+SHIFT+E: persistente Zwischenablage-Historie mit Fuzzy-Sofortsuche, Filtern (Text/Bilder/Dateien), Pins, Tastaturbedienung
- Erfassung von Text, Bildern (mit Thumbnails) und kopierten Dateipfaden; Duplikate wandern nach oben; Retention (Standard 500 Einträge)
- Lokale Verschlüsselung: AES-256-GCM pro Eintrag, Schlüssel per Windows-DPAPI geschützt; Daten unter `%USERPROFILE%\.labit\tippit\`
- Ende-zu-Ende-verschlüsselter Multi-Device-Sync über Convex ohne Account: Kopplungscode `TIPPIT-…` (+ QR), LWW-Konfliktauflösung, Tombstones, Settings-Sync, Schlüsselrotation bei Beitritt/Austritt
- Tray-Menü (Historie, Pausieren mit Blink-Icon, Sounds, Autostart über HKCU-Run-Key, Einstellungen, Beenden)
- Einstellungsfenster (Hotkeys live umbelegbar, Tippverhalten, Historie-Limits, Sync)
- NSIS-Installer
