import { describe, expect, test } from "bun:test";
import { escapeHtml, foldTerm, markMatches, renderText } from "./preview";

/** Alle Tags im Ergebnis; nur diese darf preview.ts selbst erzeugen. */
const TAG_RE = /<\/?(?<tag>[a-z]+)[^>]*>/gu;
const ALLOWED_TAGS = new Set(["mark", "a", "span"]);

function tags(html: string): string[] {
  return [...html.matchAll(TAG_RE)].map((m) => m.groups?.tag);
}

describe("escapeHtml", () => {
  test("escaped &, <, > und Anführungszeichen", () => {
    expect(escapeHtml(`<a href="x">&</a>`)).toBe(
      "&lt;a href=&quot;x&quot;&gt;&amp;&lt;/a&gt;"
    );
  });
});

describe("markMatches", () => {
  test("ohne Suchbegriff nur escapen", () => {
    expect(markMatches("<script>alert(1)</script>", "")).toBe(
      "&lt;script&gt;alert(1)&lt;/script&gt;"
    );
  });

  test("markiert Treffer und escaped den Rest", () => {
    expect(markMatches("<b>Hallo</b>", "hallo")).toBe(
      "&lt;b&gt;<mark>Hallo</mark>&lt;/b&gt;"
    );
  });

  test("Suchbegriff mitten in einer Entity zerlegt sie nicht", () => {
    const html = markMatches("Tom & Jerry", "amp");
    expect(html).toBe("Tom &amp; Jerry");
  });

  test("Sonderzeichen im Suchbegriff sind kein Regex", () => {
    expect(markMatches("a.b axb", "a.b")).toBe("<mark>a.b</mark> axb");
  });

  test("Einzelzeichen werden nicht markiert", () => {
    expect(markMatches("abc", "a")).toBe("abc");
  });

  test("gefaltet wie die Suche: Umlaute, ae und ß", () => {
    expect(markMatches("Rechnung März", "maerz")).toBe(
      "Rechnung <mark>März</mark>"
    );
    expect(markMatches("Maerz und Straße", "märz strasse")).toBe(
      "<mark>Maerz</mark> und <mark>Straße</mark>"
    );
  });

  test("zerlegte Akzente gehören zur Markierung", () => {
    expect(markMatches("Ma\u0308rz", "maerz")).toBe("<mark>Ma\u0308rz</mark>");
    expect(markMatches("Cafe\u0301 au lait", "café")).toBe(
      "<mark>Cafe\u0301</mark> au lait"
    );
    expect(markMatches("poe\u0301sie", "poésie")).toBe(
      "<mark>poe\u0301sie</mark>"
    );
  });

  test("Wortfolge in Anführungszeichen, -wort bleibt unmarkiert", () => {
    expect(markMatches("noch, einmal noch", '"noch einmal" -noch')).toBe(
      "<mark>noch, einmal</mark> noch"
    );
  });
});

/** Dieselben Fälle wie `fold_words_mirrors_frontend` in `storage/index.rs`. */
describe("foldTerm", () => {
  test("spiegelt fold_words", () => {
    expect(foldTerm("Straße")).toBe(" strasse");
    expect(foldTerm("März")).toBe(" marz");
    expect(foldTerm("MAERZ")).toBe(" marz");
    expect(foldTerm("Ma\u0308rz")).toBe(" marz");
    expect(foldTerm("Grüße, Öl")).toBe(" grusse ol");
    expect(foldTerm("Café-Bar 3")).toBe(" cafe bar 3");
    expect(foldTerm("poésie")).toBe(" poesie");
    expect(foldTerm("poe\u0301sie")).toBe(" poesie");
    expect(foldTerm("Poesie")).toBe(" posie");
    expect(foldTerm("  ")).toBe("");
  });
});

describe("renderText", () => {
  test("Skript-Tags bleiben Text", () => {
    const html = renderText("<script>alert(1)</script>", "");
    expect(html).not.toContain("<script");
    expect(tags(html)).toEqual([]);
  });

  test("Link wird escaped und nur als eigenes <a> gesetzt", () => {
    const html = renderText('siehe https://x.de/a"onmouseover=1 bitte', "");
    // Das Anführungszeichen beendet die Adresse, es kann kein Attribut öffnen.
    expect(html).toContain('data-url="https://x.de/a"');
    expect(html).toContain("&quot;onmouseover=1");
    for (const tag of tags(html)) {
      expect(ALLOWED_TAGS.has(tag)).toBe(true);
    }
  });

  test("nachlaufende Satzzeichen gehören nicht zur Adresse", () => {
    const html = renderText("Hier: https://x.de/pfad.", "");
    expect(html).toContain('data-url="https://x.de/pfad"');
    expect(html.endsWith("</a>.")).toBe(true);
  });

  test("Farbwert mit zweiter Deklaration ergibt keinen style-Ausbruch", () => {
    const html = renderText("rgb(1,2,3);background:url(x)", "");
    const styles = [...html.matchAll(/style="(?<css>[^"]*)"/gu)].map(
      (m) => m.groups?.css
    );
    expect(styles).toEqual(["background:rgb(1,2,3)"]);
  });

  test("Anführungszeichen neben Farbwert bleiben escaped", () => {
    const html = renderText('#fff" onload="x', "");
    expect(html).toContain('style="background:#fff"');
    expect(html).toContain("&quot; onload=&quot;x");
  });

  test("#abc in einer Adresse ist ein Anker, keine Farbe", () => {
    const html = renderText("https://x.de/#abc", "");
    expect(html).not.toContain("pv-swatch");
  });

  test("Suchtreffer werden auch in normalem Text markiert", () => {
    const html = renderText("Farbe #ff0000 und Text", "text");
    expect(html).toContain("<mark>Text</mark>");
    expect(html).toContain("pv-swatch");
  });
});
