# macOS-Spezifika

- Nur Apple Silicon (`aarch64-apple-darwin`): keine Intel-Targets/Universal-Builds ergänzen — Bundle-Targets/Mindestversion stehen in `src-tauri/tauri.macos.conf.json`, der Updater-Key ist `darwin-aarch64` (`release.yml`).
- Ohne Bedienungshilfen-Berechtigung verwirft macOS gepostete CGEvents STILL — „Tippen tut nichts" beim Testen heißt fast immer: fehlende AX-Freigabe für genau dieses Binary (Debug- und Release-Builds brauchen je eine eigene). `platform::ensure_input_permission` prompted beim App-Start UND als Guard vor jedem Tippversuch (`typing.rs::type_text` bricht ohne Freigabe mit Fehlerton ab) — diesen Guard nicht entfernen.
- Builds sind ad-hoc-signiert (kein Apple-Developer-Zertifikat): DMG-Downloads tragen Gatekeeper-Quarantäne (Rechtsklick → Öffnen beim Erstinstall); vom Updater entpackte Tarballs nicht.
- Bewusst KEIN Keychain für `key.bin` (ACL bindet bei Ad-hoc-Signatur an den Binary-Hash → Passwort-Prompt nach jedem Update); Schutz = 0600 + FileVault, Begründung in `platform/mac.rs`.
- Historie-Fenster bleibt eine Tauri-show/hide-Sache; die App ist Accessory (kein Dock-Icon — `LSUIElement` in `src-tauri/Info.plist` + `platform::configure_app`).
- Transparentes History/Update-Fenster: Tauri-Feature `macos-private-api` + `app.macOSPrivateApi` in `src-tauri/tauri.conf.json` — ohne Feature kompiliert `.transparent()` auf macOS nicht. Source-App: `NSWorkspace::frontmostApplication` (Bundle-ID `de.labit.tippit` = self).
