// Zentrale Registry für Eintrags-Typen und Historie-Filter.
// Neue Typen/Filter werden NUR hier ergänzt (Icon, Label, Farbe, Filterlogik) —
// die Historie-UI liest ausschließlich aus dieser Datei.
import { type EntryDto, KIND_FILES, KIND_IMAGE, KIND_TEXT } from "./api";

const URL_RE = /^https?:\/\/\S+$/i;
/** otpauth:// URIs und typische Base32-TOTP-Secrets (16–64 Zeichen). */
const OTPAUTH_RE = /^otpauth:\/\//i;
const TOTP_SECRET_RE = /^[A-Z2-7]{16,64}$/i;
const HAS_WHITESPACE_RE = /\s/;

/** Text-Eintrag, dessen Inhalt eine einzelne URL ist. */
export const isLink = (e: EntryDto) =>
  e.kind === KIND_TEXT && URL_RE.test(e.preview.trim());

/** TOTP/otpauth oder reines Base32-Secret. */
export const isTotp = (e: EntryDto) => {
  if (e.kind !== KIND_TEXT) {
    return false;
  }
  const t = e.preview.trim();
  if (OTPAUTH_RE.test(t)) {
    return true;
  }
  // Keine Leerzeichen/Zeilen: reines Secret
  return !HAS_WHITESPACE_RE.test(t) && TOTP_SECRET_RE.test(t);
};

export interface KindMeta {
  /** CSS-Variable der Typfarbe (theme.css, --kind-*). */
  colorVar: string;
  /** Icon-Name aus icon.svelte. */
  icon: string;
  /** Anzeigename in Metadaten ("Typ"). */
  label: string;
}

/** Anzeige-Metadaten eines Eintrags (Links/TOTP sind Verfeinerungen von Text). */
export function entryMeta(e: EntryDto): KindMeta {
  if (e.kind === KIND_IMAGE) {
    return { label: "Bild", icon: "image", colorVar: "var(--kind-image)" };
  }
  if (e.kind === KIND_FILES) {
    return { label: "Dateien", icon: "file", colorVar: "var(--kind-files)" };
  }
  if (isTotp(e)) {
    return { label: "TOTP", icon: "key", colorVar: "var(--kind-totp)" };
  }
  if (isLink(e)) {
    return { label: "Link", icon: "link", colorVar: "var(--kind-link)" };
  }
  return { label: "Text", icon: "text", colorVar: "var(--kind-text)" };
}

/**
 * Kontextuelle Primäraktion (Detail-Aktionsbutton + SHIFT+Enter in der Liste):
 * - `open`   — Link im Browser bzw. Datei(en) im Standard-Handler öffnen
 * - `extract`— Text aus Bild extrahieren (OCR, nur macOS)
 * - `type`   — als Tastatureingabe tippen (Text; bei TOTP der generierte Code)
 */
export type EntryAction = "open" | "extract" | "type";

export function primaryAction(e: EntryDto): EntryAction {
  if (e.kind === KIND_IMAGE) {
    return "extract";
  }
  if (e.kind === KIND_FILES || isLink(e)) {
    return "open";
  }
  return "type";
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
  {
    backendKind: KIND_TEXT,
    icon: "key",
    id: "totp",
    label: "TOTP",
    refine: isTotp,
  },
  { backendKind: KIND_FILES, icon: "file", id: "files", label: "Dateien" },
];

export type SortKey = "last_copy" | "first_copy" | "copy_count" | "size";

export function sortEntries(
  list: EntryDto[],
  key: SortKey,
  reverse: boolean
): EntryDto[] {
  const out = [...list];
  const dir = reverse ? -1 : 1;
  out.sort((a, b) => {
    // Gepinnte bleiben oben (ClipBook-ähnlich).
    if (a.pinned !== b.pinned) {
      return a.pinned ? -1 : 1;
    }
    let cmp = 0;
    switch (key) {
      case "last_copy":
        cmp = a.created_at - b.created_at;
        break;
      case "first_copy":
        cmp =
          (a.first_created_at ?? a.created_at) -
          (b.first_created_at ?? b.created_at);
        break;
      case "copy_count":
        cmp = (a.copy_count ?? 1) - (b.copy_count ?? 1);
        break;
      case "size":
        cmp = a.size_bytes - b.size_bytes;
        break;
      default:
        cmp = a.created_at - b.created_at;
        break;
    }
    // Neueste / größte zuerst
    cmp = -cmp;
    return cmp * dir;
  });
  return out;
}
