import { describe, expect, test } from "bun:test";
import { type EntryDto, KIND_FILES, KIND_IMAGE, KIND_TEXT } from "./api";
import {
  entryMeta,
  FILTERS,
  isLink,
  isTotp,
  primaryAction,
  sortEntries,
} from "./entry-kinds";

function entry(over: Partial<EntryDto>): EntryDto {
  return {
    copy_count: 1,
    created_at: 0,
    first_created_at: 0,
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

describe("isLink", () => {
  test("einzelne http(s)-Adresse", () => {
    expect(isLink(entry({ preview: " https://example.org/a?b=1 " }))).toBe(
      true
    );
    expect(isLink(entry({ preview: "http://x.de" }))).toBe(true);
  });

  test("Text mit Adresse, andere Schemes und Bilder sind keine Links", () => {
    expect(isLink(entry({ preview: "siehe https://x.de" }))).toBe(false);
    expect(isLink(entry({ preview: "ftp://x.de" }))).toBe(false);
    expect(isLink(entry({ kind: KIND_IMAGE, preview: "https://x.de" }))).toBe(
      false
    );
  });
});

describe("isTotp", () => {
  test("otpauth-URI und rohes Base32-Secret", () => {
    expect(isTotp(entry({ preview: "otpauth://totp/x?secret=ABC" }))).toBe(
      true
    );
    expect(isTotp(entry({ preview: "JBSWY3DPEHPK3PXP" }))).toBe(true);
  });

  test("zu kurz, mit Leerzeichen oder falsches Alphabet", () => {
    expect(isTotp(entry({ preview: "JBSWY3DP" }))).toBe(false);
    expect(isTotp(entry({ preview: "JBSWY3DP EHPK3PXP" }))).toBe(false);
    expect(isTotp(entry({ preview: "JBSWY3DPEHPK3PX1" }))).toBe(false);
    expect(
      isTotp(entry({ kind: KIND_FILES, preview: "JBSWY3DPEHPK3PXP" }))
    ).toBe(false);
  });
});

describe("entryMeta und primaryAction", () => {
  test("Baustein geht vor dem Inhaltstyp", () => {
    expect(
      entryMeta(entry({ snippet: true, preview: "https://x.de" })).label
    ).toBe("Baustein");
  });

  test("Primäraktion je Typ", () => {
    expect(primaryAction(entry({ kind: KIND_IMAGE }))).toBe("extract");
    expect(primaryAction(entry({ kind: KIND_FILES }))).toBe("open");
    expect(primaryAction(entry({ preview: "https://x.de" }))).toBe("open");
    expect(primaryAction(entry({ preview: "JBSWY3DPEHPK3PXP" }))).toBe("type");
    expect(primaryAction(entry({ preview: "Hallo" }))).toBe("type");
  });
});

describe("FILTERS", () => {
  test("ids sind eindeutig, der erste Filter zeigt alles", () => {
    const ids = FILTERS.map((f) => f.id);
    expect(new Set(ids).size).toBe(ids.length);
    expect(FILTERS[0].backendKind).toBeNull();
    expect(FILTERS[0].refine).toBeUndefined();
  });
});

describe("sortEntries", () => {
  const list = [
    entry({ uuid: "alt", created_at: 1, size_bytes: 50, copy_count: 3 }),
    entry({ uuid: "neu", created_at: 3, size_bytes: 10, copy_count: 1 }),
    entry({ uuid: "pin", created_at: 0, pinned: true, size_bytes: 1 }),
    entry({ uuid: "snip", created_at: 0, snippet: true, size_bytes: 1 }),
    entry({
      uuid: "mitte",
      created_at: 2,
      first_created_at: 5,
      size_bytes: 30,
      copy_count: 2,
    }),
  ];
  const order = (key: Parameters<typeof sortEntries>[1], rev = false) =>
    sortEntries(list, key, rev).map((e) => e.uuid);

  test("Bausteine vor Angepinntem vor dem Rest, Neuestes zuerst", () => {
    expect(order("last_copy")).toEqual(["snip", "pin", "neu", "mitte", "alt"]);
  });

  test("Umkehren betrifft nur den Rest", () => {
    expect(order("last_copy", true)).toEqual([
      "snip",
      "pin",
      "alt",
      "mitte",
      "neu",
    ]);
  });

  test("erste Kopierzeit, Anzahl und Größe", () => {
    expect(order("first_copy")).toEqual(["snip", "pin", "mitte", "alt", "neu"]);
    expect(order("copy_count")).toEqual(["snip", "pin", "alt", "mitte", "neu"]);
    expect(order("size")).toEqual(["snip", "pin", "alt", "mitte", "neu"]);
  });

  test("Eingabe bleibt unverändert", () => {
    const before = list.map((e) => e.uuid);
    sortEntries(list, "size", false);
    expect(list.map((e) => e.uuid)).toEqual(before);
  });
});
