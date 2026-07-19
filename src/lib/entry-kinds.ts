// Zentrale Registry für Eintrags-Typen und Historie-Filter.
// Neue Typen/Filter werden NUR hier ergänzt (Icon, Label, Farbe, Filterlogik) —
// die Historie-UI liest ausschließlich aus dieser Datei.
import { type EntryDto, KIND_FILES, KIND_IMAGE, KIND_TEXT } from "./api";

const URL_RE = /^https?:\/\/\S+$/i;

/** Text-Eintrag, dessen Inhalt eine einzelne URL ist. */
export const isLink = (e: EntryDto) =>
  e.kind === KIND_TEXT && URL_RE.test(e.preview.trim());

export interface KindMeta {
  /** CSS-Variable der Typfarbe (theme.css, --kind-*). */
  colorVar: string;
  /** Icon-Name aus icon.svelte. */
  icon: string;
  /** Anzeigename in Metadaten ("Typ"). */
  label: string;
}

/** Anzeige-Metadaten eines Eintrags (Links sind eine Verfeinerung von Text). */
export function entryMeta(e: EntryDto): KindMeta {
  if (e.kind === KIND_IMAGE) {
    return { label: "Bild", icon: "image", colorVar: "var(--kind-image)" };
  }
  if (e.kind === KIND_FILES) {
    return { label: "Dateien", icon: "file", colorVar: "var(--kind-files)" };
  }
  if (isLink(e)) {
    return { label: "Link", icon: "link", colorVar: "var(--kind-link)" };
  }
  return { label: "Text", icon: "text", colorVar: "var(--kind-text)" };
}

/** CSS-Variable der leichten Hintergrund-Tönung einer Listenzeile. */
export function entryTintVar(e: EntryDto): string {
  return entryMeta(e).colorVar.replace(")", "-tint)");
}

export interface HistoryFilter {
  /** kind-Filter, den das Backend anwendet (null = alle). */
  backendKind: number | null;
  icon: string;
  id: string;
  label: string;
  /** Optionale clientseitige Verfeinerung des Backend-Ergebnisses. */
  refine?: (e: EntryDto) => boolean;
}

export const FILTERS: HistoryFilter[] = [
  { backendKind: null, icon: "clipboard", id: "all", label: "Alle" },
  {
    backendKind: null,
    icon: "star",
    id: "pinned",
    label: "Favoriten",
    refine: (e) => e.pinned,
  },
  { backendKind: KIND_TEXT, icon: "text", id: "text", label: "Text" },
  { backendKind: KIND_IMAGE, icon: "image", id: "image", label: "Bilder" },
  {
    backendKind: KIND_TEXT,
    icon: "link",
    id: "links",
    label: "Links",
    refine: isLink,
  },
  { backendKind: KIND_FILES, icon: "file", id: "files", label: "Dateien" },
];
