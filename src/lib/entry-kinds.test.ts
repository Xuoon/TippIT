import { describe, expect, test } from "bun:test";
import { type EntryDto, KIND_FILES, KIND_IMAGE, KIND_TEXT } from "./api";
import {
  displayPreview,
  entryMeta,
  FILTERS,
  isLink,
  isTotp,
  primaryAction,
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

// Dieselben Fälle prüft `refine_mirrors_frontend` in src-tauri/src/storage/index.rs.
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

  test("Base32-Padding am Ende", () => {
    expect(isTotp(entry({ preview: "JBSWY3DPEHPK3PXPJBSW====" }))).toBe(true);
    expect(isTotp(entry({ preview: "JBSWY3DP========" }))).toBe(false);
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

describe("displayPreview", () => {
  test("verdeckt TOTP-Secrets, auch rohe", () => {
    expect(
      displayPreview({
        kind: KIND_TEXT,
        preview: "otpauth://totp/A?secret=ABC&issuer=A",
      })
    ).toBe("otpauth://totp/A?secret=••••&issuer=A");
    expect(
      displayPreview({ kind: KIND_TEXT, preview: "JBSWY3DPEHPK3PXP" })
    ).toBe("••••••••");
    expect(
      displayPreview({ kind: KIND_TEXT, preview: "JBSWY3DPEHPK3PXPJBSW====" })
    ).toBe("••••••••");
  });

  test("anderer Text bleibt", () => {
    expect(displayPreview({ kind: KIND_TEXT, preview: "JBSWY3DP" })).toBe(
      "JBSWY3DP"
    );
    expect(
      displayPreview({ kind: KIND_FILES, preview: "JBSWY3DPEHPK3PXP" })
    ).toBe("JBSWY3DPEHPK3PXP");
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
