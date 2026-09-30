import { describe, expect, test } from "bun:test";
import { type EntryDto, KIND_TEXT } from "../api";
import { sortEntries } from "../entry-kinds";
import { dateGroupLabel } from "./grouping";

// Feste Ortszeit, damit die Tagesgrenzen nicht vom Testlauf abhängen.
const NOW = new Date(2026, 8, 30, 14, 0, 0);

function at(y: number, m: number, d: number, h = 12): number {
  return new Date(y, m, d, h).getTime();
}

function entry(over: Partial<EntryDto>): EntryDto {
  return {
    copy_count: 1,
    created_at: NOW.getTime(),
    first_created_at: NOW.getTime(),
    has_html: false,
    has_thumb: false,
    kind: KIND_TEXT,
    pinned: false,
    preview: "",
    size_bytes: 0,
    snippet: false,
    source_app_id: null,
    source_app_name: null,
    uuid: "u",
    ...over,
  };
}

const label = (over: Partial<EntryDto>) =>
  dateGroupLabel(entry(over), "last_copy", NOW);

describe("dateGroupLabel", () => {
  test("Tagesgrenzen", () => {
    expect(label({ created_at: at(2026, 8, 30, 0) })).toBe("Heute");
    expect(label({ created_at: at(2026, 8, 29, 23) })).toBe("Gestern");
    expect(label({ created_at: at(2026, 8, 24) })).toBe("Letzte 7 Tage");
    expect(label({ created_at: at(2026, 8, 2) })).toBe("Dieser Monat");
    expect(label({ created_at: at(2026, 7, 31) })).toBe("August 2026");
  });

  test("Zeitstempel in der Zukunft zählt als heute", () => {
    expect(label({ created_at: at(2026, 9, 2) })).toBe("Heute");
  });

  test("erste Kopierzeit nutzt first_created_at", () => {
    const e = entry({
      created_at: NOW.getTime(),
      first_created_at: at(2026, 8, 29),
    });
    expect(dateGroupLabel(e, "first_copy", NOW)).toBe("Gestern");
    expect(dateGroupLabel(e, "last_copy", NOW)).toBe("Heute");
  });

  test("Bausteine und Angepinntes haben eigene Gruppen", () => {
    expect(label({ snippet: true })).toBe("Textbausteine");
    expect(label({ snippet: true, pinned: true })).toBe("Textbausteine");
    expect(label({ pinned: true })).toBe("Angepinnt");
  });

  test("nach Sortierung erscheint jede Überschrift genau einmal", () => {
    const list = sortEntries(
      [
        entry({ uuid: "a" }),
        entry({ uuid: "b", snippet: true }),
        entry({ uuid: "c", pinned: true }),
        entry({ uuid: "d", created_at: at(2026, 8, 29) }),
        entry({ uuid: "e", snippet: true, created_at: at(2026, 5, 1) }),
      ],
      "last_copy",
      false
    );
    const heads: string[] = [];
    for (const e of list) {
      const l = dateGroupLabel(e, "last_copy", NOW);
      if (heads.at(-1) !== l) {
        heads.push(l);
      }
    }
    expect(heads).toEqual(["Textbausteine", "Angepinnt", "Heute", "Gestern"]);
  });
});
