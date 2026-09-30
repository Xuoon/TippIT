import { describe, expect, test } from "bun:test";
import {
  groupTone,
  parseChangelog,
  releaseDate,
  releaseName,
} from "./changelog";

const SAMPLE = `# Changelog

Vorspann ohne Release wird ignoriert.

## [Unreleased]

### Hinzugefügt

- **Neu**: siehe [Doku](https://example.com) und \`code\`

## [2.1.0] – 2026-09-23

Kurzfassung des Release.

### Behoben

- Erster Fix
- Zweiter Fix
`;

describe("parseChangelog", () => {
  const releases = parseChangelog(SAMPLE);

  test("erkennt Releases in Dateireihenfolge", () => {
    expect(releases.map((r) => r.title)).toEqual([
      "Unreleased",
      "2.1.0 – 2026-09-23",
    ]);
  });

  test("entfernt Markdown-Auszeichnung aus Punkten", () => {
    expect(releases[0].groups[0]).toEqual({
      items: ["Neu: siehe Doku und code"],
      title: "Hinzugefügt",
    });
  });

  test("trennt Einleitung und Gruppenpunkte", () => {
    expect(releases[1].intro).toEqual(["Kurzfassung des Release."]);
    expect(releases[1].groups[0].items).toEqual(["Erster Fix", "Zweiter Fix"]);
  });
});

describe("Release-Titel", () => {
  test("Version und Datum", () => {
    expect(releaseName("2.1.0 – 2026-09-23")).toBe("2.1.0");
    expect(releaseDate("2.1.0 – 2026-09-23")).toBe("2026-09-23");
  });

  test("Unreleased ohne Datum", () => {
    expect(releaseName("Unreleased")).toBe("Unveröffentlicht");
    expect(releaseDate("Unreleased")).toBe("");
  });
});

test("groupTone kennt nur die vier Standardgruppen", () => {
  expect(groupTone("Behoben ")).toBe("fix");
  expect(groupTone("Sicherheit")).toBe("");
});
