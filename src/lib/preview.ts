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
const ESCAPE_RE = /[&<>"]/g;

export const escapeHtml = (s: string) =>
  s.replace(ESCAPE_RE, (c) => ESCAPES[c]);

/** http(s)-Adressen — bewusst nur diese beiden Schemes, wie `open_link`. */
const URL_RE = /https?:\/\/[^\s<>"']+/g;
/** #rgb, #rrggbb, #rrggbbaa sowie rgb()/rgba()/hsl()/hsla(). Innerhalb der
    Klammern sind bewusst NUR Zahlen und Trennzeichen erlaubt: der Wert landet in
    einem style-Attribut und darf dort keine zweite Deklaration aufmachen. */
const COLOR_RE =
  /#(?:[0-9a-f]{3}|[0-9a-f]{6}|[0-9a-f]{8})\b|(?:rgb|hsl)a?\([\d\s.,%/]{3,40}\)/gi;

interface Segment {
  kind: "color" | "link" | "text";
  value: string;
}

/** Nachlaufende Satzzeichen gehören zum Satz, nicht zur Adresse. */
const TRAILING_RE = /[.,;:!?)\]}]+$/;

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

function queryParts(query: string): string[] {
  return [
    ...new Set(
      query
        .trim()
        .split(WHITESPACE_RE)
        .filter((p) => p.length >= 2)
    ),
  ]
    .sort((a, b) => b.length - a.length)
    .slice(0, 8);
}

const REGEX_SPECIAL_RE = /[.*+?^${}()|[\]\\]/g;
const WHITESPACE_RE = /\s+/;

/**
 * Suchtreffer im Klartext markieren. Die Suche selbst ist unscharf (nucleo),
 * markiert werden deshalb die wörtlichen Vorkommen der Suchbegriffe — was
 * nicht wörtlich vorkommt, bleibt schlicht unmarkiert.
 */
export function markMatches(text: string, query: string): string {
  const parts = queryParts(query);
  if (parts.length === 0) {
    return escapeHtml(text);
  }
  const pattern = new RegExp(
    `(${parts.map((p) => p.replace(REGEX_SPECIAL_RE, "\\$&")).join("|")})`,
    "gi"
  );
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
