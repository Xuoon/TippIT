// Reine Helfer der Mini-Palette (Route src/routes/palette).
import type { PaletteModifier } from "./api";

/** Ist genau dieser Modifier beim Klick oder Tastendruck gehalten? `cmd` ist
    unter Windows die Windows-Taste, beides meldet der Browser als metaKey. */
export function modifierHeld(
  event: Pick<KeyboardEvent, "altKey" | "ctrlKey" | "metaKey" | "shiftKey">,
  modifier: PaletteModifier
): boolean {
  switch (modifier) {
    case "alt":
      return event.altKey;
    case "ctrl":
      return event.ctrlKey;
    case "cmd":
      return event.metaKey;
    default:
      return event.shiftKey;
  }
}

const DIGIT_CODE_RE = /^(?:Digit|Numpad)([1-9])$/;

/** Index (0-basiert) zu einer Zifferntaste 1–9 per `event.code`: mit Shift
    liefert `event.key` sonst „!" statt „1". */
export function digitIndex(code: string, count: number): number | null {
  const match = DIGIT_CODE_RE.exec(code);
  if (!match) {
    return null;
  }
  const index = Number(match[1]) - 1;
  return index < count ? index : null;
}
