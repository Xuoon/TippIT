// Zentrale Registry für Eintrags-Typen und Historie-Filter.
// Neue Typen/Filter werden NUR hier ergänzt (Icon, Label, Farbe, Filterlogik) —
// die Historie-UI liest ausschließlich aus dieser Datei.
import {
  type EntryDto,
  KIND_FILES,
  KIND_IMAGE,
  KIND_TEXT,
  type RefineId,
} from "./api";
import { maskOtpauthSecret } from "./totp";

const URL_RE = /^https?:\/\/\S+$/i;
/** otpauth:// URIs und typische Base32-TOTP-Secrets (16–64 Zeichen). */
const OTPAUTH_RE = /^otpauth:\/\//i;
const TOTP_SECRET_RE = /^[A-Z2-7]{16,64}$/i;
const HAS_WHITESPACE_RE = /\s/;

// isLink und isTotp spiegelt `is_link`/`is_totp` in src-tauri/src/storage/index.rs,
// das die Filter auswertet; Änderungen an beiden Stellen samt Tests.

/** Text-Eintrag, dessen Inhalt eine einzelne URL ist. */
export const isLink = (e: EntryDto) =>
  e.kind === KIND_TEXT && URL_RE.test(e.preview.trim());

/** TOTP/otpauth oder reines Base32-Secret. */
export const isTotp = (e: Pick<EntryDto, "kind" | "preview">) => {
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

/**
 * Anzeigetext für Liste und Papierkorb: bei TOTP ohne Secret, wie in der
 * Detailansicht bis „Secret anzeigen". Ein rohes Secret wird ganz verdeckt.
 */
export function displayPreview(e: Pick<EntryDto, "kind" | "preview">): string {
  if (!isTotp(e)) {
    return e.preview;
  }
  return OTPAUTH_RE.test(e.preview.trim())
    ? maskOtpauthSecret(e.preview)
    : "••••••••";
}

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
  if (e.snippet) {
    return {
      label: "Baustein",
      icon: "bookmark",
      colorVar: "var(--kind-snippet)",
    };
  }
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
 * - `extract`— Text aus Bild extrahieren (OCR)
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
  /** Verfeinerung über den Typ hinaus, ebenfalls im Backend ausgewertet. */
  refine?: RefineId;
}

export const FILTERS: HistoryFilter[] = [
  { backendKind: null, icon: "clipboard", id: "all", label: "Alle" },
  {
    backendKind: null,
    icon: "star",
    id: "pinned",
    label: "Favoriten",
    refine: "pinned",
  },
  {
    backendKind: null,
    icon: "bookmark",
    id: "snippets",
    label: "Bausteine",
    refine: "snippets",
  },
  { backendKind: KIND_TEXT, icon: "text", id: "text", label: "Text" },
  { backendKind: KIND_IMAGE, icon: "image", id: "image", label: "Bilder" },
  {
    backendKind: KIND_TEXT,
    icon: "link",
    id: "links",
    label: "Links",
    refine: "links",
  },
  {
    backendKind: KIND_TEXT,
    icon: "key",
    id: "totp",
    label: "TOTP",
    refine: "totp",
  },
  { backendKind: KIND_FILES, icon: "file", id: "files", label: "Dateien" },
];
