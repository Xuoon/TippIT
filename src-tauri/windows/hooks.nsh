; TippIT NSIS-Hooks: sauberes Updaten (Drüber-Installieren) und Deinstallieren.

!macro NSIS_HOOK_PREINSTALL
  ; Laufende TippIT-Instanz hart beenden, damit die EXE ersetzt werden kann
  ; (Tray-App ohne Fenster lässt sich vom Standard-Close des Installers nicht schließen).
  nsExec::Exec 'taskkill /F /IM tippit.exe'
  Sleep 500
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  nsExec::Exec 'taskkill /F /IM tippit.exe'
  Sleep 500
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; Autostart-Eintrag entfernen, den tauri-plugin-autostart gesetzt hat —
  ; sonst bleibt nach der Deinstallation ein toter Run-Key zurück.
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "TippIT"
  ; Nutzerdaten unter %USERPROFILE%\.labi\tippit bleiben bewusst erhalten.
!macroend
