import { describe, expect, test } from "bun:test";
import { detectLanguage, highlight, LANGUAGES } from "./highlight";

/** Jedes `<` im Ergebnis muss ein eigenes Token-<span> sein. */
const OWN_TAG_RE = /^<(?:span class="tok-[a-z-]+">|\/span>)/;

function onlyOwnTags(html: string): boolean {
  let idx = html.indexOf("<");
  while (idx !== -1) {
    if (!OWN_TAG_RE.test(html.slice(idx))) {
      return false;
    }
    idx = html.indexOf("<", idx + 1);
  }
  return true;
}

const HOSTILE = [
  `<script>alert("x")</script>`,
  `<img src=x onerror='alert(1)'>`,
  `const a = "<b>" + '</b>'; // <i>`,
  `/* <style> */ SELECT '<x>' FROM t; -- <y>`,
  `<!-- <iframe> --> <div class="a&b">&amp;</div>`,
  `# <svg onload=1>\necho "<a>"`,
  `"unterminated <b`,
].join("\n");

describe("highlight", () => {
  for (const lang of LANGUAGES) {
    test(`${lang.id}: Quelltext bleibt escaped`, () => {
      const html = highlight(HOSTILE, lang.id);
      expect(onlyOwnTags(html)).toBe(true);
      expect(html).not.toContain("<script");
      expect(html).not.toContain("<img");
    });
  }

  test("ohne Sprache nur escapen", () => {
    expect(highlight("<b>&</b>", null)).toBe("&lt;b&gt;&amp;&lt;/b&gt;");
  });

  test("Schlüsselwörter und Zeichenketten werden eingefärbt", () => {
    const html = highlight('const x = "a";', "ts");
    expect(html).toContain('<span class="tok-keyword">const</span>');
    expect(html).toContain('<span class="tok-string">&quot;a&quot;</span>');
  });
});

describe("detectLanguage", () => {
  test("erkennt JSON", () => {
    expect(detectLanguage('{\n  "a": 1\n}')).toBe("json");
  });

  test("Fließtext ist kein Code", () => {
    expect(
      detectLanguage(
        "Hallo zusammen,\nanbei die Notizen vom Termin.\nViele Grüße"
      )
    ).toBeNull();
  });
});
