import type { SemanticStatus } from "#lib/api.ts";
import { fmtBytes } from "#lib/format.ts";

/** Fortschritt in Prozent, null ohne messbaren Fortschritt. */
export function phasePercent(status: SemanticStatus): number | null {
  if (status.phase !== "downloading" && status.phase !== "indexing") {
    return null;
  }
  return status.total > 0
    ? Math.min(100, Math.round((status.done / status.total) * 100))
    : 0;
}

/** Ein Satz zum Zustand, für die Statuszeile. */
export function phaseText(status: SemanticStatus): string {
  switch (status.phase) {
    case "off":
      return "Aus";
    case "downloading":
      return `Modell wird geladen: ${fmtBytes(status.done)} von ${fmtBytes(status.total)}`;
    case "loading":
      return "Modell wird gestartet…";
    case "indexing":
      return `Einträge werden eingelesen: ${status.done} von ${status.total}`;
    case "error":
      return `Fehler: ${status.message}`;
    default:
      return status.loaded
        ? "Bereit"
        : "Bereit, Modell ruht (startet beim Öffnen der Historie)";
  }
}
