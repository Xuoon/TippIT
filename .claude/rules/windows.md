# Windows-Spezifika

- Sichtbarkeit des Historie-Fensters läuft in `platform/win.rs` über Win32 (`ShowWindow`/`IsWindowVisible`), NICHT über Tauris show/hide/is_visible — das Fenster kann ohne Aktivierung gezeigt worden sein, wovon Tauris interner Visible-Zustand nichts mitbekommt (hide() wäre dann ein No-op, Fenster „stuck").
- `key.bin` ist DPAPI-gewrappt (User-Scope, `platform/win.rs`): nicht auf andere Maschinen/Benutzer kopierbar; Datenumzug nur über Kopplungscode/Sync.
- NSIS-Bundling mit „os error 5": Toolset manuell nach `%LOCALAPPDATA%\tauri\NSIS` legen (inkl. `Plugins/x86-unicode/additional/nsis_tauri_utils.dll`).
- `ctrl+shift+escape` ist als Hotkey nicht registrierbar (Task-Manager, Fehler 1409) — bei der Hotkey-Aufnahme in den Einstellungen möglich, Registrierung scheitert dann still ins Log.
- Source-App: Vordergrund via `GetForegroundWindow` → Prozess-Pfad (`QueryFullProcessImageNameW`); Self = gleiches Image wie `current_exe` bzw. `tippit.exe`. App-spezifische Windows-Icons sind noch nicht implementiert (generischer UI-Fallback). Transparentes History-Fenster: bei Hit-Test-Problemen an den Ecken prüfen (CSS-Radius + `.transparent(true)`).
