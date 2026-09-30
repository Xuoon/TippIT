import type { EntryDto, SortKey } from "../api";

const DAY_MS = 86_400_000;
/** toLocaleString kostet pro Aufruf spürbar; bei tausenden Zeilen je Monat einmal. */
const monthLabels = new Map<number, string>();

/**
 * Gruppenlabel eines Eintrags; aufeinanderfolgende gleiche Label teilen einen
 * Header. Bausteine und Angepinntes stehen in der Sortierung vorn und bekommen
 * deshalb eigene Gruppen, sonst zerrissen sie die Datumsgruppen.
 */
export function dateGroupLabel(
  e: EntryDto,
  sortKey: SortKey,
  now: Date = new Date()
): string {
  if (e.snippet) {
    return "Textbausteine";
  }
  if (e.pinned) {
    return "Angepinnt";
  }
  const ts =
    sortKey === "first_copy"
      ? (e.first_created_at ?? e.created_at)
      : e.created_at;
  const day = new Date(ts);
  day.setHours(0, 0, 0, 0);
  const today = new Date(now);
  today.setHours(0, 0, 0, 0);
  const diffDays = Math.round((today.getTime() - day.getTime()) / DAY_MS);
  if (diffDays <= 0) {
    return "Heute";
  }
  if (diffDays === 1) {
    return "Gestern";
  }
  if (diffDays < 7) {
    return "Letzte 7 Tage";
  }
  const d = new Date(ts);
  if (
    d.getFullYear() === today.getFullYear() &&
    d.getMonth() === today.getMonth()
  ) {
    return "Dieser Monat";
  }
  const key = d.getFullYear() * 12 + d.getMonth();
  let label = monthLabels.get(key);
  if (label === undefined) {
    label = d.toLocaleString("de-DE", { month: "long", year: "numeric" });
    monthLabels.set(key, label);
  }
  return label;
}
