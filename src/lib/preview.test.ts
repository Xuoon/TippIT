import { describe, expect, test } from "bun:test";
import { escapeHtml, markMatches, renderText } from "./preview";

/** Alle Tags im Ergebnis; nur diese darf preview.ts selbst erzeugen. */
const TAG_RE = /<\/?([a-z]+)[^>]*>/g;
const ALLOWED_TAGS = new Set(["mark", "a", "span"]);

function tags(html: string): string[] {
  return [...html.matchAll(TAG_RE)].map((m) => m[1]);
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
    const styles = [...html.matchAll(/style="([^"]*)"/g)].map((m) => m[1]);
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
