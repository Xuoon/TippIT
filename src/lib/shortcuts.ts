import { primaryModifierLabel } from "./platform";

export interface Shortcut {
  keys: string;
  label: string;
  /** Auch in der Hilfe der Einstellungen zeigen. */
  settings?: boolean;
}

/** Tastenkürzel der Historie; einzige Quelle für Historie und Einstellungen. */
export const SHORTCUTS: Shortcut[] = [
  {
    keys: "Enter / Doppelklick",
    label: "Ins Zielfenster einfügen",
    settings: true,
  },
  {
    keys: `${primaryModifierLabel}+Enter`,
    label: "Ins Zielfenster tippen",
    settings: true,
  },
  {
    keys: `${primaryModifierLabel}+Doppelklick`,
    label: "Zeichenweise tippen",
    settings: true,
  },
  {
    keys: "⇧+Enter",
    label: "Primäraktion (Link öffnen, Text aus Bild lesen)",
    settings: true,
  },
  {
    keys: `${primaryModifierLabel}+1…9`,
    label: "n-ten Eintrag direkt einfügen",
    settings: true,
  },
  { keys: "↑ / ↓", label: "Auswahl bewegen", settings: true },
  { keys: "Tab", label: "Filter wechseln", settings: true },
  { keys: "Tippen", label: "Sucht sofort" },
  {
    keys: `${primaryModifierLabel}+P`,
    label: "Anpinnen / lösen",
    settings: true,
  },
  {
    keys: `${primaryModifierLabel}+B`,
    label: "Als Textbaustein merken",
    settings: true,
  },
  {
    keys: `${primaryModifierLabel}+Entf / ⇧+Entf`,
    label: "In den Papierkorb",
    settings: true,
  },
  { keys: "?", label: "Diese Übersicht" },
  { keys: "Esc", label: "Fenster schließen", settings: true },
];
