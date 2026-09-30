// Palette in der Hotkey-Leiste: Modifier-Auswahl und Beschriftung.
import type { PaletteModifier } from "../../api";

/** Unter Windows fehlt ⌘: die Win-Taste öffnet das Startmenü. */
export function paletteModifiers(isMac: boolean): PaletteModifier[] {
  return isMac ? ["alt", "ctrl", "cmd", "shift"] : ["alt", "ctrl", "shift"];
}

/** PARITÄT: dieselben Symbole wie `formatHotkey` (platform.ts). */
export function modifierLabel(modifier: PaletteModifier, isMac: boolean) {
  const mac: Record<PaletteModifier, string> = {
    alt: "⌥",
    cmd: "⌘",
    ctrl: "⌃",
    shift: "⇧",
  };
  const win: Record<PaletteModifier, string> = {
    alt: "ALT",
    cmd: "WIN",
    ctrl: "STRG",
    shift: "SHIFT",
  };
  return (isMac ? mac : win)[modifier];
}

/** Öffnen und Tippen brauchen verschiedene Modifier, sonst tippte jeder
    Palettenklick. Wählbar ist daher alles außer dem jeweils anderen. */
export function modifierChoices(
  isMac: boolean,
  other: PaletteModifier
): { disabled: boolean; value: PaletteModifier }[] {
  return paletteModifiers(isMac).map((value) => ({
    disabled: value === other,
    value,
  }));
}

/** Anzahl der Einträge (Rust: 1 bis 9); ein krummer gespeicherter Wert bleibt
    wählbar, damit er im Menü markiert ist. */
export function paletteCounts(current: number): number[] {
  const steps = [3, 5, 7, 9];
  return steps.includes(current)
    ? steps
    : [...steps, current].sort((a, b) => a - b);
}
