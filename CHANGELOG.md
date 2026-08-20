# Changelog

Alle nennenswerten Änderungen an TippIT. Format angelehnt an [Keep a Changelog](https://keepachangelog.com/de/1.1.0/), Versionierung nach [SemVer](https://semver.org/lang/de/).

## [2.0.0] – 2026-08-20

TippIT arbeitet ab dieser Version ausschließlich auf Ihrem Gerät. Die Synchronisierung zwischen Geräten und der Server dahinter sind entfallen; an ihre Stelle tritt eine verschlüsselte Sicherungsdatei für den Umzug. Dazu kommen Textbausteine, ein Papierkorb, farbige Code-Vorschau und eine Historie, die auch mit tausenden Einträgen flüssig bleibt.

### Hinzugefügt

- **Textbausteine**: Häufig gebrauchte Texte lassen sich dauerhaft ablegen — Signatur, Adresse, Standardantworten. Sie stehen ganz oben in der Historie, haben einen eigenen Filter und überstehen sowohl das Eintragslimit als auch „Historie löschen". Die Platzhalter `{datum}`, `{uhrzeit}` und `{datumzeit}` werden beim Tippen durch das aktuelle Datum ersetzt. Anlegen über den Baustein-Filter, ein vorhandener Eintrag wird per Strg/⌘+B zum Baustein
- **Papierkorb**: Gelöschte Einträge landen 30 Tage im Papierkorb statt sofort verloren zu gehen. Erreichbar über das Papierkorb-Symbol links in der Historie, dort einzeln wiederherstellbar oder endgültig zu entfernen
- **Sicherung exportieren und importieren** (Einstellungen → Daten): Die Historie lässt sich in eine passwortgeschützte Datei schreiben und auf einem anderen Rechner wieder einlesen. Der Import führt zusammen, statt zu überschreiben — vorhandene Einträge bleiben. Das ist der Weg, TippIT auf einen neuen Rechner mitzunehmen
- **Farbige Code-Vorschau**: Erkennt die Vorschau Code, färbt sie ihn ein — JSON, JavaScript/TypeScript, Rust, Python, SQL, Shell, CSS und HTML. Die erkannte Sprache steht in den Details und lässt sich im Vorschaubereich umstellen, falls die Erkennung danebenliegt
- **Formatierung bleibt erhalten**: Kopierter Text mit Farben, Fettungen oder Tabellen behält seine Formatierung und wird beim Einfügen wieder formatiert übergeben. Die Vorschau zeigt beides — formatiert und als Klartext. Getippt wird weiterhin reiner Text. Abschaltbar unter Einstellungen → Historie
- **Anklickbare Adressen und Farbproben in der Vorschau**: Enthaltene http(s)-Adressen öffnen sich per Klick, erkannte Farbwerte wie `#3b82f6` bekommen ein Farbfeld daneben
- **Suchtreffer werden hervorgehoben** — in der Trefferliste und in der Vorschau
- **Automatisches Aufräumen** (Einstellungen → Historie): Einträge, die älter als die eingestellte Frist sind, wandern in den Papierkorb. Angepinntes und Textbausteine bleiben unberührt. Standard: aus
- **Programme ausschließen** (Einstellungen → Historie): Aus eingetragenen Programmen wird gar nichts erfasst — für Passwortmanager oder Banking. Es genügt der Programmname, etwa „KeePass"
- **Strg/⌘+1 bis 9** tippt den ersten bis neunten Eintrag der Historie direkt, ohne Navigieren
- **Tastenkürzel-Übersicht** in der Historie: `?` oder das Fragezeichen links zeigt alle Kürzel
- **Bild als PNG speichern**: Bild-Einträge haben eine Speichern-Aktion im Vorschaubereich
- **Umfang eines Eintrags** in den Details: Zeichen, Wörter und Zeilen
- **Fortschritt beim Update**: Der Update-Hinweis unten rechts zeigt jetzt den Versionssprung, einen Fortschrittsbalken mit Prozentwert und die geladene Datenmenge, danach den Installationsschritt; auf macOS schließt ein „Jetzt neu starten"-Knopf den Vorgang ab. Der Update-Knopf in den Einstellungen füllt sich währenddessen mit demselben Fortschritt
- Ein Klick auf den Log-Pfad in der Hilfe öffnet das Datenverzeichnis im Dateimanager

### Geändert

- **Die Historie bleibt auch bei sehr vielen Einträgen flüssig**: Gezeichnet wird nur noch, was tatsächlich zu sehen ist
- **Standard-Hotkeys wieder STRG+E (⌘E) und STRG+SHIFT+E (⌘⇧E)**; eigene Belegungen bleiben unangetastet
- **Doppelklick in der Historie fügt den Eintrag direkt** in das Fenster ein, das vor dem Öffnen der Historie aktiv war — der ganze Text erscheint auf einmal; mit gedrückter Strg/⌘-Taste wird er stattdessen zeichenweise getippt (Bilder werden weiterhin nur kopiert)
- **Esc bricht jetzt überall ab**: auch während TippIT auf das Zielfenster wartet oder den Startton spielt, und im Update-Hinweis sowie in den Einstellungen schließt Esc das Fenster
- **„Historie löschen" ist nicht mehr endgültig** — die Einträge liegen danach im Papierkorb
- Die Einstellungen haben statt des Sync-Tabs einen Tab **Daten** mit Sicherung und Datenverzeichnis; „Was ist neu?" zeigt Neuerungen, Änderungen, Entferntes und Behobenes jetzt farblich unterschieden
- Hilfe und „Was ist neu?" füllen das Einstellungsfenster jetzt ganz aus; im Changelog ist nur der neueste Eintrag aufgeklappt, die Hilfe ist auf die Kurzbefehle und einen Hinweis für den Fall „nichts passiert" zusammengestrichen

### Entfernt

- **Synchronisierung zwischen Geräten**, Kopplungscodes, Gruppen und die Geräteliste. TippIT braucht damit keinen Server mehr und sendet nichts ins Netz — außer der Prüfung auf Programm-Updates. Ihre Historie bleibt weiterhin verschlüsselt auf dem Gerät gespeichert; für den Wechsel auf einen anderen Rechner gibt es jetzt Export und Import
- Die Sync-Einstellungen (Umfang, Intervall, Server-Adresse und die Richtlinien für Mobilfunk-, Energiespar- und Datensparmodus) entfallen ersatzlos

### Behoben

- Selbst gesetzte Hotkeys wurden bei jedem Start wieder auf den Standard zurückgesetzt

## [1.1.1] – 2026-07-29

### Geändert

- **Enter tippt den gewählten Historieneintrag direkt** in das Fenster beziehungsweise Textfeld, das vor dem Öffnen der Historie aktiv war
- Ein ruhender Mauszeiger wählt beim Öffnen der Historie keinen darunterliegenden Eintrag mehr aus — die Hover-Auswahl reagiert erst auf eine tatsächliche Mausbewegung

### Behoben

- Globale Tipp- und Historien-Hotkeys funktionieren unter Windows jetzt auch dann, wenn Vordergrundprogramme wie TeamViewer sie abfangen
- Das Historienfenster wird gegenüber Vordergrund- und Always-on-top-Fenstern zuverlässiger eingeblendet und behält den Tastaturfokus

## [1.1.0] – 2026-07-23

Y-Hotkeys mit macOS-Layout-Fix, neu gedachte Einstellungen, Windows-Parität bei OCR und App-Icons, QR-Codes und eine Geräteliste für den Sync.

### Hinzugefügt

- **OCR jetzt auch unter Windows** (eingebaute Windows-Texterkennung): Text aus Bild-Einträgen extrahieren inkl. markierbarem Overlay — erkannt wird, was als Windows-Sprachpaket installiert ist
- **Quellanwendungs-Icons unter Windows**: Historie-Einträge zeigen das echte EXE-Icon der Quellanwendung statt des generischen Platzhalters
- **QR-Codes lesen**: Bild-Einträge haben eine QR-Aktion im Detail-Bereich — dekodierte Inhalte lassen sich kopieren oder (bei Links) direkt öffnen; ein kopierter `otpauth://`-Code landet als TOTP-Eintrag mit Live-Code in der Historie
- **Historie optional nach Datum gruppiert** (Sortiermenü → „Nach Datum gruppieren"): klebende Zwischenüberschriften Angepinnt/Heute/Gestern/Letzte 7 Tage/Dieser Monat/Monat Jahr bei Zeit-Sortierung

### Geändert

- Standard-Hotkeys nutzen jetzt **Y statt E**: STRG+Y (⌘Y) tippt, STRG+SHIFT+Y (⌘⇧Y) öffnet die Historie
- **Einstellungen komplett überarbeitet:** Die beiden Hotkeys stehen als große, klickbare Tastenkappen ganz oben (Klick nimmt direkt neu auf), darunter vier Tabs (Allgemein, Tippen, Historie, Sync) statt einer langen Scroll-Seite. Die Suche flacht alle Tabs zu einer Trefferliste ab; die Hilfe ist ein Overlay hinter dem ?-Knopf, Version, „Was ist neu?" und Update-Prüfung wohnen in der Fußzeile
- **Geräte der Sync-Gruppe** sichtbar: Einstellungen → Sync listet alle Geräte mit Name, Plattform und „zuletzt aktiv"; das eigene Gerät ist markiert

### Entfernt

- **Settings-Sync**: Programm-Einstellungen bleiben jetzt komplett geräte-lokal — der Sync überträgt nur noch Historie-Einträge

### Behoben

- Favoriten-Stern in der Historie-Liste sitzt bei Bild-Einträgen jetzt rechtsbündig
- Hotkey-Registrierung auf macOS ging von der physischen US-Tastatur aus: auf QWERTZ reagierten Hotkeys mit Y auf die Z-Taste (und umgekehrt)
- Das Einstellungsfenster erschien im macOS-App-Switcher (⌘Tab) nur als icon-loser „Geist"

## [1.0.0] – 2026-07-23

Erstes stabiles Release mit macOS-Unterstützung, überarbeiteter Historie und Aktionen pro Eintrag.

### Hinzugefügt

- macOS-Unterstützung (Apple Silicon): ⌘E tippt die Zwischenablage, ⌘⇧E öffnet die Historie — als Menüleisten-App ohne Dock-Icon, mit DMG-Download und signierten In-App-Updates. macOS fragt beim ersten Start die Bedienungshilfen-Berechtigung ab; ohne sie wird jeder Tippversuch mit Fehlerton abgebrochen
- Historie und Einstellungen zeigen plattformgerechte Kurzbefehle (⌘ auf dem Mac, Strg unter Windows); auf dem Mac entfallen die dort wirkungslosen Mobilfunk- und Datensparmodus-Schalter
- Hilfe-Bereich in den Einstellungen: Kurzbefehle und Problembehebung auf einen Blick
- Einstellungen zeigen die installierte Version mit Update-Knopf; „Was ist neu?" öffnet das Changelog direkt in der App
- **Quellanwendung** pro Historie-Eintrag (geräte-lokal, nicht im Sync): Name auf beiden Plattformen, App-Icon auf macOS; Ziel-App im Footer für „In … einfügen“
- Historie-**Sortierung** (Letzte/Erste Kopierzeit, Anzahl der Kopien, Größe, Reihenfolge umkehren)
- **TOTP**: eigener Filter/Farbe (`otpauth://` und typische Base32-Secrets) und in der Vorschau der **live generierte Code mit Countdown**; Kopieren und Tippen erzeugen den aktuellen Code statt des Secrets
- **Aktionen pro Eintrag** direkt aus der Historie: **Links im Browser öffnen**, **Datei(en) im Standard-Programm öffnen**, Text aus Bild extrahieren (OCR) — im Detail-Bereich und per **⇧+Enter** in der Liste
- **OCR auf macOS:** Text aus Bild-Einträgen extrahieren — direkt im Bild markierbar (Live-Text-Overlay auf den erkannten Zeilen) und zusätzlich als kopierbarer Text in der Vorschau
- Vorschau- und Details-Bereich in der Historie ein-/ausblendbar; **Listenspalte per Ziehen** in der Breite anpassbar

### Geändert

- Tippen abbrechen ist jetzt immer **Esc** (auch schon in der Startverzögerung) — der separate, umbelegbare Abbruch-Hotkey entfällt
- Hotkeys werden nicht mehr zwischen Geräten synchronisiert — sie bleiben geräte-lokal, weil die Belegungen plattformspezifisch sind (Strg vs. ⌘)
- **Historie flacher gestaltet:** Zeilen als kantige, durch Haarlinien getrennte Liste statt Karten
- Bild-Vorschau nutzt die volle Breite und lädt das Bild in voller Auflösung — nicht mehr unscharf
- Historie- und Update-Fenster als gerundetes Floating-Panel

### Behoben

- Schlüsselrotation ist gegen gleichzeitige Zugriffe abgesichert und bricht atomar ab, sobald ein Eintrag nicht umschlüsselbar ist
- Sync verliert keine Einträge mehr nach Entschlüsselungs- oder Formatfehlern

### Entfernt

- Direkt-Kopieren der ersten neun Einträge per Strg/⌘+1…9 inkl. der Badges in den Listenzeilen (zugunsten einer ruhigeren Liste)

## [0.5.0] – 2026-07-20

### Hinzugefügt

- Größe des Historie-Fensters in den Einstellungen einstellbar (70–150 %, Doppelklick auf den Slider = Standard 100 %)

### Geändert

- Historie öffnet immer mit leerer Suche und Standard-Filter

### Behoben

- Hotkey-Aufnahme in den Einstellungen ging vom US-Tastaturlayout aus: auf QWERTZ wurden Y und Z vertauscht angezeigt und registriert

## [0.4.0] – 2026-07-20

### Hinzugefügt

- Abbruch-Hotkey (Standard STRG+ALT+ESC, umbelegbar): bricht einen laufenden Tipp-Vorgang ab
- Tray-Icon blinkt während des Tippens; der Tipp-Hotkey wird bei geöffneter Historie ignoriert
- Helles Theme mit Live-Umschaltung: System/Dunkel/Hell wirkt sofort in allen Fenstern, ohne Neustart
- Historie dreispaltig: Kategorien-Leiste (Alle, Favoriten, Text, Bilder, Links, Dateien), einzeilige Liste und große Vorschau mit Metadaten
- Sofort-Suche in der Historie: Lostippen startet die Suche
- Einstellungs-Suche hebt Treffer farblich hervor; Slider springen per Doppelklick auf ihren Standardwert

### Geändert

- Historie-Fenster erhält beim Öffnen den Fokus — Pfeiltasten und Suche funktionieren sofort
- Einstellungen: feste Fenstergröße, Auswahl als kompakte Pillen, breiteres Server-URL-Feld

## [0.3.0] – 2026-07-19

### Hinzugefügt

- Signierter In-App-Updater mit automatischer Prüfung beim Start, dezentem Hinweis unten rechts und manueller Update-Prüfung in den Einstellungen
- Sync-Richtlinien für Mobilfunk, Windows-Energiesparmodus und Datensparmodus sowie wählbarer Sync-Abstand (sofort bis stündlich)

### Geändert

- Einstellungen als kompakte Ein-Seiten-Ansicht; das Fenster zeigt keinen weißen Start-Frame mehr
- Historie flacher, kompakter und farblich ruhiger gestaltet
- Datenverzeichnis direkt auf `%USERPROFILE%\.labi\tippit\` umgestellt

### Entfernt

- MSI-Installer — ausgeliefert wird nur noch das Setup (`.exe`)

## [0.2.3] – 2026-07-19

### Behoben

- Schlüsselrotation (Beitritt/Austritt/neuer Code) konnte bei einem Fehler die gesamte Historie unlesbar machen
- „Als Tastatur tippen" tippte nach fixer Wartezeit blind — jetzt wird geprüft, ob das Zielfenster wirklich im Vordergrund ist; sonst Abbruch mit Fehlerton statt Eingabe ins falsche Fenster
- Das Geräte-Limit „max. Einträge" stutzte die Historie aller gekoppelten Geräte auf das kleinste Limit — es wirkt jetzt nur noch lokal
- Nicht dekodierbare Sync-Einträge konnten dauerhaft verloren gehen
- Texte mit reinen `\r`-Zeilenumbrüchen (Alt-Mac, manche Excel-Exporte) wurden als eine zusammengeklebte Zeile getippt
- Historie-Fenster: veraltete Suchantworten konnten neuere überschreiben; fehlgeschlagene Thumbnails blieben dauerhaft leer
- Einstellungen: gerade getippte Feld-Eingaben (z. B. Server-URL) wurden beim Speichern zurückgesetzt

## [0.2.2] – 2026-07-17

### Geändert

- Zeichenweises Tippen ist jetzt der Standard-Modus (funktioniert zuverlässig auch in RDP/Citrix); „Alles auf einmal" bleibt wählbar

## [0.2.1] – 2026-07-17

### Behoben

- Historie-Fenster ließ sich nach dem fokuslosen Öffnen nicht mehr schließen (✕/Hotkey wirkungslos)
- Update-Installation über eine laufende TippIT-Instanz schlug fehl bzw. verlangte manuelles Deinstallieren
- Deinstallation räumt jetzt den Autostart-Eintrag mit auf (Nutzerdaten bleiben erhalten)

## [0.2.0] – 2026-07-17

### Hinzugefügt

- Vorschau-Panel in der Historie: mehrzeiliger Text-Viewer bzw. vergrößerte Bildvorschau für den ausgewählten Eintrag
- Schließen-Button (✕) im Historie-Fenster
- Sync-Tab: „Code kopieren", „Code + QR anzeigen" und „Neuen Code erstellen" (mit Bestätigung; trennt ggf. die aktive Gruppe und verschlüsselt die Historie um)
- Sync-Server-URL ist ab Werk vorausgefüllt

### Geändert

- Historie öffnet ohne Fokusklau — das zuvor aktive Fenster (z. B. Windows-Suche) bleibt offen und fokussiert
- Historie bleibt offen, bis sie aktiv geschlossen wird (✕, Esc oder Hotkey); kein automatisches Verstecken bei Fokusverlust mehr
- Server-URL in den Sync-Einstellungen unter „Erweitert" verschoben
- Drüber-Installieren ersetzt die alte Version, Daten bleiben erhalten

## [0.1.0] – 2026-07-16

Kompletter Neubau der AutoIt-App als Windows-Anwendung.

### Hinzugefügt

- STRG+E: Zwischenablage als literale Tastatureingaben tippen (Bulk- oder Zeichenweise-Modus, konfigurierbare Verzögerung)
- STRG+SHIFT+E: persistente Zwischenablage-Historie mit Fuzzy-Sofortsuche, Filtern (Text/Bilder/Dateien), Pins, Tastaturbedienung
- Erfassung von Text, Bildern (mit Thumbnails) und kopierten Dateipfaden; Duplikate wandern nach oben; Retention (Standard 500 Einträge)
- Lokale Verschlüsselung: AES-256-GCM pro Eintrag, Schlüssel per Windows-DPAPI geschützt
- Ende-zu-Ende-verschlüsselter Multi-Device-Sync ohne Account: Kopplungscode `TIPPIT-…` (+ QR)
- Tray-Menü (Historie, Pausieren, Sounds, Autostart, Einstellungen, Beenden)
- Einstellungsfenster (Hotkeys live umbelegbar, Tippverhalten, Historie-Limits, Sync)
- NSIS-Installer
