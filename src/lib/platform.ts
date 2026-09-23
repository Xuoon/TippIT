/**
 * Einzige Plattformweiche im Frontend — alle Komponenten beziehen
 * Modifier-Verhalten und Hotkey-Beschriftung von hier.
 */
export const isMacOS = navigator.userAgent.includes("Mac");

/**
 * Fenster-Chrome von Historie und Update-Hinweis: Windows zeichnet sie opak
 * mit nativen Ecken und Schatten, macOS transparent mit eigenem Rahmen
 * (Parität zu `platform::TRANSPARENT_WINDOW` in win.rs/mac.rs). Setzt
 * `data-chrome` am <html>, die Routen-CSS richtet sich danach.
 */
export function applyWindowChrome(): void {
  document.documentElement.dataset.chrome = isMacOS ? "floating" : "native";
}

/** Primärer App-Modifier gedrückt? ⌘ (metaKey) auf macOS, Strg sonst. */
export function primaryModifierPressed(
  event: KeyboardEvent | MouseEvent
): boolean {
  return isMacOS ? event.metaKey : event.ctrlKey;
}

/** Beschriftung des primären Modifiers für Labels/Tooltips. */
export const primaryModifierLabel = isMacOS ? "⌘" : "Strg";

/**
 * Hotkey-String (z. B. "cmd+shift+e") fürs UI formatieren.
 * PARITÄT: Symbole und Token müssen deckungsgleich mit der Tray-Anzeige in
 * src-tauri/src/platform/{win,mac}.rs::display_hotkey bleiben.
 */
export function formatHotkey(hotkey: string): string {
  const upper = hotkey.toUpperCase();
  if (isMacOS) {
    return upper
      .replace("SUPER", "⌘")
      .replace("CMD", "⌘")
      .replace("CTRL", "⌃")
      .replace("SHIFT", "⇧")
      .replace("ALT", "⌥")
      .replaceAll("+", " + ");
  }
  return upper.replace("CTRL", "STRG").replaceAll("+", " + ");
}
