// Aufbereitung von Klartext für Liste und Detail-Vorschau.
//
// Alle Funktionen liefern HTML und escapen den Eingabetext dafür selbst; in das
// Ergebnis kommen ausschließlich hier erzeugte Tags. Fremdes HTML läuft NIE
// durch diese Datei — dafür ist die Rust-Sanitisierung zuständig.

const ESCAPES: Record<string, string> = {
  "&": "&amp;",
  "<": "&lt;",
  ">": "&gt;",
  '"': "&quot;",
};
const ESCAPE_RE = /[&<>"]/gu;

export const escapeHtml = (s: string) =>
  s.replace(ESCAPE_RE, (c) => ESCAPES[c]);

/** http(s)-Adressen — bewusst nur diese beiden Schemes, wie `open_link`. */
const URL_RE = /https?:\/\/[^\s<>"']+/gu;
/** #rgb, #rrggbb, #rrggbbaa sowie rgb()/rgba()/hsl()/hsla(). Innerhalb der
    Klammern sind bewusst NUR Zahlen und Trennzeichen erlaubt: der Wert landet in
    einem style-Attribut und darf dort keine zweite Deklaration aufmachen. */
const COLOR_RE =
  /#(?:[0-9a-f]{3}|[0-9a-f]{6}|[0-9a-f]{8})\b|(?:rgb|hsl)a?\([\d\s.,%/]{3,40}\)/giu;

interface Segment {
  kind: "color" | "link" | "text";
  value: string;
}

/** Nachlaufende Satzzeichen gehören zum Satz, nicht zur Adresse. */
const TRAILING_RE = /[.,;:!?)\]}]+$/u;

function segment(text: string): Segment[] {
  const segments: Segment[] = [];
  const marks: { end: number; kind: "color" | "link"; start: number }[] = [];

  URL_RE.lastIndex = 0;
  let m = URL_RE.exec(text);
  while (m !== null) {
    const trimmed = m[0].replace(TRAILING_RE, "");
    marks.push({ start: m.index, end: m.index + trimmed.length, kind: "link" });
    m = URL_RE.exec(text);
  }
  COLOR_RE.lastIndex = 0;
  m = COLOR_RE.exec(text);
  while (m !== null) {
    const start = m.index;
    const end = start + m[0].length;
    // Innerhalb einer Adresse ist „#abc" ein Anker, keine Farbe.
    if (!marks.some((k) => start >= k.start && start < k.end)) {
      marks.push({ start, end, kind: "color" });
    }
    m = COLOR_RE.exec(text);
  }
  marks.sort((a, b) => a.start - b.start);

  let pos = 0;
  for (const mark of marks) {
    if (mark.start < pos) {
      continue;
    }
    if (mark.start > pos) {
      segments.push({ kind: "text", value: text.slice(pos, mark.start) });
    }
    segments.push({ kind: mark.kind, value: text.slice(mark.start, mark.end) });
    pos = mark.end;
  }
  if (pos < text.length) {
    segments.push({ kind: "text", value: text.slice(pos) });
  }
  return segments;
}

const WORD_SEP = "[^\\p{L}\\p{N}]+";
const COMBINING_RE = /[\u0300-\u036f]/u;
const ALNUM_RE = /[\p{L}\p{N}]/u;
const FOLD: Record<string, string> = {
  ä: "a",
  à: "a",
  á: "a",
  â: "a",
  ã: "a",
  å: "a",
  ö: "o",
  ò: "o",
  ó: "o",
  ô: "o",
  õ: "o",
  ø: "o",
  ü: "u",
  ù: "u",
  ú: "u",
  û: "u",
  è: "e",
  é: "e",
  ê: "e",
  ë: "e",
  ì: "i",
  í: "i",
  î: "i",
  ï: "i",
  ç: "c",
  ñ: "n",
  ý: "y",
  ÿ: "y",
  ß: "ss",
};
/** Was im Text auf ein gefaltetes Zeichen zurückgeht. */
const UNFOLD: Record<string, string> = {
  e: "[eèéêë]",
  i: "[iìíîï]",
  c: "[cç]",
  n: "[nñ]",
  y: "[yýÿ]",
};

/**
 * Spiegel von `fold_words` (`src-tauri/src/storage/index.rs`): Kleinbuchstaben
 * ohne Akzente, ä/ae → a, ö/oe → o, ü/ue → u, ß → ss; jedes Wort mit
 * führendem Leerzeichen, alles außer Buchstaben und Ziffern trennt.
 */
export function foldTerm(text: string): string {
  let out = "";
  let inWord = false;
  let afterVowel = false;
  // Ein e nach a/o/u wartet: mit kombinierendem Akzent ist es ein é und bleibt.
  let heldE = false;
  for (const c of text.toLowerCase()) {
    if (COMBINING_RE.test(c)) {
      if (heldE) {
        out += "e";
        heldE = false;
      }
      continue;
    }
    heldE = false;
    if (!ALNUM_RE.test(c)) {
      inWord = false;
      afterVowel = false;
      continue;
    }
    if (!inWord) {
      out += " ";
      inWord = true;
    }
    if (c === "e" && afterVowel) {
      afterVowel = false;
      heldE = true;
      continue;
    }
    afterVowel = c === "a" || c === "o" || c === "u";
    out += FOLD[c] ?? c;
  }
  return out;
}

/** Kombinierende Akzente, die die Faltung übergeht (zerlegtes Unicode). */
const MARKS = "[\\u0300-\\u036f]*";
/** Ein schlichtes e, das nach a/o/u wegfällt (ohne folgenden Akzent). */
const PLAIN_E = "(?:e(?![\\u0300-\\u036f]))?";
/** Was im Text auf ein gefaltetes a/o/u zurückgeht. */
const UNFOLD_VOWEL: Record<string, string> = {
  a: `(?:a${MARKS}${PLAIN_E}|[äàáâãå])`,
  o: `(?:o${MARKS}${PLAIN_E}|[öòóôõø])`,
  u: `(?:u${MARKS}${PLAIN_E}|[üùúû])`,
};

/** Regex für einen gefalteten Begriff (nur Buchstaben, Ziffern, Leerzeichen):
    trifft jede Schreibweise, die Rust gleich faltet, Akzente eingeschlossen. */
function unfoldPattern(folded: string): string {
  const words = folded.trim().split(" ");
  return words
    .map((word) => {
      let re = "";
      for (let i = 0; i < word.length; i++) {
        if (word[i] === "s" && word[i + 1] === "s") {
          re += "(?:ss|ß)";
          i++;
        } else {
          re += UNFOLD_VOWEL[word[i]] ?? UNFOLD[word[i]] ?? word[i];
        }
        re += MARKS;
      }
      return re;
    })
    .join(WORD_SEP);
}

/** Markierbare Teile der Suche: Wörter und `"…"`-Sätze, ohne `-wort`. */
function queryParts(query: string): string[] {
  const parts: string[] = [];
  query.split('"').forEach((chunk, i) => {
    if (i % 2 === 1) {
      parts.push(chunk);
      return;
    }
    for (const word of chunk.split(WHITESPACE_RE)) {
      if (!word.startsWith("-")) {
        parts.push(word);
      }
    }
  });
  return [...new Set(parts.map(foldTerm))]
    .filter((p) => p.replaceAll(" ", "").length >= 2)
    .sort((a, b) => b.length - a.length)
    .slice(0, 8)
    .map(unfoldPattern);
}

const WHITESPACE_RE = /\s+/u;

/**
 * Suchtreffer im Klartext markieren, gefaltet wie die Suche in Rust („maerz“
 * markiert „März“). Tippfehler und Treffer im App-Namen bleiben unmarkiert.
 */
export function markMatches(text: string, query: string): string {
  const parts = queryParts(query);
  if (parts.length === 0) {
    return escapeHtml(text);
  }
  const pattern = new RegExp(`(${parts.join("|")})`, "giu");
  // Gesucht wird im ROHTEXT und erst danach escaped: auf dem escapten Text
  // träfe ein Suchbegriff wie „amp" mitten in eine Entity und zerlegte sie.
  let out = "";
  let last = 0;
  for (const m of text.matchAll(pattern)) {
    out += escapeHtml(text.slice(last, m.index));
    out += `<mark>${escapeHtml(m[0])}</mark>`;
    last = m.index + m[0].length;
  }
  return out + escapeHtml(text.slice(last));
}

/**
 * Detail-Vorschau für gewöhnlichen Text: Adressen werden anklickbar, erkannte
 * Farbwerte bekommen eine Farbprobe, Suchtreffer werden markiert.
 * Der Klick auf eine Adresse wird in der Historie abgefangen (`data-url`), das
 * Öffnen selbst macht weiterhin Rust.
 */
export function renderText(text: string, query: string): string {
  return segment(text)
    .map((seg) => {
      if (seg.kind === "link") {
        const safe = escapeHtml(seg.value);
        return `<a class="pv-link" data-url="${safe}" href="#">${safe}</a>`;
      }
      if (seg.kind === "color") {
        const safe = escapeHtml(seg.value);
        return `<span class="pv-color"><span class="pv-swatch" style="background:${safe}"></span>${safe}</span>`;
      }
      return markMatches(seg.value, query);
    })
    .join("");
}
