---
paths:
  - "src-tauri/src/platform/mac.rs"
  - "src-tauri/src/typing.rs"
  - "src-tauri/src/hotkeys.rs"
  - "src-tauri/src/windows_util.rs"
  - "src-tauri/tauri.macos.conf.json"
  - "src-tauri/Info.plist"
  - ".github/workflows/release.yml"
---

# macOS-Spezifika

- Nur Apple Silicon (`aarch64-apple-darwin`): keine Intel-Targets/Universal-Builds ergänzen — Bundle-Targets/Mindestversion stehen in `src-tauri/tauri.macos.conf.json`, der Updater-Key ist `darwin-aarch64` (`release.yml`).
- Ohne Bedienungshilfen-Berechtigung verwirft macOS gepostete CGEvents STILL — „Tippen tut nichts" beim Testen heißt fast immer: fehlende AX-Freigabe für genau dieses Binary (Debug- und Release-Builds brauchen je eine eigene). Den AX-Guard in `typing.rs::type_text` nicht entfernen.
- Builds sind ad-hoc-signiert (kein Apple-Developer-Zertifikat): DMG-Downloads tragen Gatekeeper-Quarantäne (Erstinstall: bis macOS 14 Rechtsklick → Öffnen, ab macOS 15 Systemeinstellungen → Datenschutz & Sicherheit → „Dennoch öffnen"); vom Updater entpackte Tarballs nicht.
- Bewusst KEIN Keychain für `key.bin` (ACL bindet bei Ad-hoc-Signatur an den Binary-Hash → Passwort-Prompt nach jedem Update), Begründung in `platform/mac.rs`.
- Die App ist Accessory (kein Dock-Icon, `LSUIElement` + `platform::configure_app`). Solange das Einstellungsfenster offen ist, schaltet `platform::set_app_switcher_visible` auf `Regular`, sonst wäre das Fenster im ⌘Tab-Switcher ein icon-loser „Geist"; beim Schließen zurück auf Accessory.
- Das global-shortcut-Plugin registriert Buchstaben als PHYSISCHE US-Keycodes — auf QWERTZ wären Y/Z vertauscht. `platform::resolve_hotkey` übersetzt das gespeicherte Zeichen deshalb bei jeder Registrierung layoutbewusst; Settings/Anzeige führen weiter das getippte Zeichen (Windows: Identität).
