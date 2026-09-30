// Hotkey-Aufnahme der Einstellungen: Tastendruck → gespeicherter Hotkey-String
// ("ctrl+shift+e"), wie ihn hotkeys.rs registriert.

type KeyInput = Pick<
  KeyboardEvent,
  "altKey" | "code" | "ctrlKey" | "key" | "metaKey" | "shiftKey"
>;

const F_KEY_PATTERN = /^F\d{1,2}$/i;
const LETTER_DIGIT_PATTERN = /^[a-z0-9]$/;

// Layoutbewusst über event.key: event.code liefert die PHYSISCHE Taste im
// US-Layout, auf QWERTZ wären Y und Z vertauscht. Registriert wird ebenfalls
// layoutbewusst (Windows: virtuelle Keys; macOS: platform::resolve_hotkey).
function keyFromEvent(event: KeyInput): string | null {
  const key = event.key.toLowerCase();
  if (LETTER_DIGIT_PATTERN.test(key) || F_KEY_PATTERN.test(event.key)) {
    return key;
  }
  // Shift+Ziffer liefert als key ein Sonderzeichen ("!", "§", …),
  // dann hilft der physische Code weiter.
  if (event.code.startsWith("Digit")) {
    return event.code.slice(5);
  }
  return null;
}

/** Hotkey-String aus einem Tastendruck; null, solange Modifier oder eine
    unterstützte Taste (Buchstabe, Ziffer, F-Taste) fehlen. */
export function hotkeyFromEvent(event: KeyInput): string | null {
  const key = keyFromEvent(event);
  if (!key) {
    return null;
  }
  const parts: string[] = [];
  if (event.ctrlKey) {
    parts.push("ctrl");
  }
  if (event.shiftKey) {
    parts.push("shift");
  }
  if (event.altKey) {
    parts.push("alt");
  }
  if (event.metaKey) {
    parts.push("super");
  }
  if (parts.length === 0) {
    return null;
  }
  parts.push(key);
  return parts.join("+");
}

/** Gleiches Kürzel trotz anderer Schreibweise ("cmd+shift+e" = "shift+super+e")? */
export function sameHotkey(a: string, b: string): boolean {
  const normalize = (hotkey: string) =>
    hotkey
      .toLowerCase()
      .split("+")
      .map((part) => (part === "cmd" || part === "command" ? "super" : part))
      .sort()
      .join("+");
  return normalize(a) === normalize(b);
}
