# Changelog

Alle nennenswerten Änderungen an TippIT. Format angelehnt an [Keep a Changelog](https://keepachangelog.com/de/1.1.0/), Versionierung nach [SemVer](https://semver.org/lang/de/).

## [1.0.0] – 2026-07-23

Erstes stabiles Release mit macOS-Unterstützung, überarbeiteter Historie und Aktionen pro Eintrag.

### Hinzugefügt

- macOS-Unterstützung (Apple Silicon): ⌘E tippt die Zwischenablage, ⌘⇧E öffnet die Historie — als Menüleisten-App ohne Dock-Icon, mit DMG-Download und signierten In-App-Updates. macOS fragt beim ersten Start die Bedienungshilfen-Berechtigung ab; ohne sie wird jeder Tippversuch mit Fehlerton abgebrochen und der Dialog erneut ausgelöst
- Historie und Einstellungen zeigen plattformgerechte Kurzbefehle (⌘ auf dem Mac, Strg unter Windows); auf dem Mac entfallen die dort wirkungslosen Mobilfunk- und Datensparmodus-Schalter
- Hilfe-Bereich in den Einstellungen: Schnellstart, alle Tastaturkürzel der Historie und Problembehebung auf einen Blick
- Einstellungen zeigen unten in der Seitenleiste die installierte Version mit Update-Knopf; „Was ist neu?" öffnet das mitgelieferte Changelog direkt in der App
- **Quellanwendung** pro Historie-Eintrag (geräte-lokal, nicht im Sync): Name auf beiden Plattformen, App-Icon auf macOS und generischer Fallback unter Windows; Meta-Zeile „Anwendung“; Ziel-App im Footer für „In … einfügen“
- Historie-**Sortierung** (Letzte/Erste Kopierzeit, Anzahl der Kopien, Größe, Reihenfolge umkehren)
- **TOTP**: eigener Filter/Farbe (`otpauth://` und typische Base32-Secrets) und in der Vorschau der **live generierte Code mit Countdown**; Kopieren und Tippen erzeugen den aktuellen Code statt des Secrets
- **Aktionen pro Eintrag** direkt aus der Historie: **Links im Browser öffnen**, **Datei(en) im Standard-Programm öffnen**, Text aus Bild extrahieren (OCR) — im Detail-Bereich und per **⇧+Enter** in der Liste; Enter kopiert, ⌘/Strg+Enter tippt
- **OCR auf macOS:** Text aus Bild-Einträgen über Vision extrahieren — direkt im Bild markierbar (Live-Text-Overlay auf den erkannten Zeilen) und zusätzlich als kopierbarer Text in der Vorschau, mit Scan-Animation; unter Windows noch nicht verfügbar
- Vorschau- und Details-Bereich in der Historie ein-/ausblendbar; **Listenspalte per Ziehen** in der Breite anpassbar (Einstellungen bleiben pro Gerät im Browser-Speicher)

### Geändert

- Tippen abbrechen ist jetzt immer **Esc** (während eines laufenden Tipp-Vorgangs, auch schon in der Startverzögerung) — der separate, umbelegbare Abbruch-Hotkey entfällt
- Hotkeys werden nicht mehr zwischen Geräten synchronisiert — sie bleiben geräte-lokal, weil die Belegungen plattformspezifisch sind (Strg vs. ⌘)
- **Historie flach überarbeitet:** Zeilen als kantige, durch Haarlinien getrennte Liste statt Karten, deckende Neutral-Selektion ohne farbigen Rand/Strich, Detail-Bereiche über Versal-Labels („Details", „Extrahierter Text") getrennt
- Bild-Vorschau nutzt die volle Breite und lädt das Bild in voller Auflösung (statt des Listen-Thumbnails) — nicht mehr unscharf
- Historie- und Update-Fenster als gerundetes Floating-Panel (transparenter Fensterhintergrund, nativer Eckenradius auf macOS)
- Settings-Sync überträgt nur noch eine versionierte Allowlist; Hotkeys, Server-URL, Zeitplan und Systemrichtlinien verlassen das Gerät nicht

### Behoben

- Schlüsselrotation ist gegen gleichzeitige Clipboard-, History- und Sync-Zugriffe abgesichert und bricht atomar ab, sobald ein Ciphertext nicht umschlüsselbar ist
- Sync verliert keine Einträge mehr durch Cursor-Fortschritt nach Entschlüsselungs-/Formatfehlern; Push/Pull-Batches beachten Byte-Limits und Subscription-Fehler starten die Session neu
- Noch nicht gepushte Settings-Änderungen bleiben über App-Neustarts erhalten; Scope-Erweiterungen berücksichtigen zuvor lokale Einträge erneut

### Entfernt

- Direkt-Kopieren der ersten neun Einträge per Strg/⌘+1…9 inkl. der Badges in den Listenzeilen (bewusst zugunsten einer ruhigeren Liste)

## [0.5.0] – 2026-07-20

### Hinzugefügt

- Größe des Historie-Fensters in den Einstellungen einstellbar (70–150 %, Doppelklick auf den Slider = Standard 100 %)

### Geändert

- Historie öffnet immer mit leerer Suche und Standard-Filter; Enter oder Doppelklick kopiert den Eintrag und schließt das Fenster
- Sync-Umfang (Text & Dateipfade, Einstellungen, Bilder) als kompakte Pillen wie bei den Systemrichtlinien

### Behoben

- Hotkey-Aufnahme in den Einstellungen ging vom US-Tastaturlayout aus: auf QWERTZ wurden Y und Z vertauscht angezeigt und registriert

## [0.4.0] – 2026-07-20

### Hinzugefügt

- Abbruch-Hotkey (Standard STRG+ALT+ESC, umbelegbar): bricht einen laufenden Tipp-Vorgang ab — auch schon während der Startverzögerung
- Tray-Icon blinkt während des Tippens (grüner Punkt); STRG+E wird bei geöffneter Historie ignoriert (Schutz vor Tippen ins eigene Fenster)
- Helles Theme mit Live-Umschaltung: System/Dunkel/Hell wirkt sofort in allen Fenstern, ohne Neustart
- Historie im dreispaltigen ClipBook-Stil: Kategorien-Leiste (Alle, Favoriten, Text, Bilder, Links, Dateien), einzeilige Liste mit dezenter Typ-Färbung, große Vorschau mit Metadaten (Typ, Größe, Kopierzeit)
- Sofort-Suche in der Historie: Lostippen startet die Suche; Doppelklick kopiert den Eintrag und schließt das Fenster
- Einstellungs-Suche hebt Treffer farblich hervor; Slider springen per Doppelklick auf ihren Standardwert

### Geändert

- Historie-Fenster erhält beim Öffnen den Fokus — Pfeiltasten und Suche funktionieren sofort
- Einstellungen: feste Fenstergröße, Auswahl als kompakte Pillen (Darstellung, Tipp-Modus, Systemrichtlinien), breiteres Server-URL-Feld, Sync-Verbindung ohne QR-Code und ohne doppelte Aktionen
- Auslieferungs-Standardwerte kommen im Frontend zentral über den neuen Command `default_settings` (keine duplizierten Defaults mehr)

## [0.3.0] – 2026-07-19

### Hinzugefügt

- Signierter In-App-Updater mit automatischer Prüfung beim Start, dezentem Hinweis unten rechts und manueller Update-Prüfung in den Einstellungen
- Sync-Richtlinien für Mobilfunk, Windows-Energiesparmodus und Datensparmodus sowie wählbarer Sync-Abstand (sofort bis stündlich)

### Geändert

- Einstellungen als kompakte Ein-Seiten-Ansicht ohne Seitennavigation; das Fenster wird erst nach dem Laden eingeblendet und zeigt keinen weißen Start-Frame mehr
- Historie flacher, kompakter und farblich ruhiger gestaltet
- Datenverzeichnis direkt auf `%USERPROFILE%\.labi\tippit\` umgestellt; `.labi` erhält unter Windows das Hidden-Attribut
- Releases enthalten nur noch das NSIS-Setup, dessen Updater-Signatur und `latest.json`; MSI wurde vollständig aus dem aktuellen Build- und Release-Prozess entfernt

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
