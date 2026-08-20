// Syntax-Hervorhebung für die Detail-Vorschau.
//
// Bewusst ohne Bibliothek: ein Zwischenablage-Schnipsel ist selten länger als
// ein paar hundert Zeilen, und eine eigene, knappe Tokenisierung kostet weder
// Bundle-Größe noch Startzeit. Der erzeugte HTML-String enthält AUSSCHLIESSLICH
// selbst gesetzte <span>-Tags — der Quelltext wird vorher escaped, sonst wäre
// die Vorschau ein Einfallstor.

/** Ab dieser Größe bleibt der Text ungefärbt: Hervorhebung darf die Vorschau
    nie spürbar verzögern. */
export const HIGHLIGHT_MAX_CHARS = 100_000;

export interface Language {
  id: string;
  label: string;
}

interface LangDef extends Language {
  /** Blockkommentar-Paare. */
  block: [string, string][];
  keywords: Set<string>;
  /** Zeilenkommentar-Präfixe. */
  line: string[];
  /** Auszeichnung als Markup (Tags/Attribute) statt als Programmtext. */
  markup?: boolean;
  /** Zeichen, die eine Zeichenkette eröffnen (und sie wieder schließen). */
  quotes: string[];
}

const def = (
  id: string,
  label: string,
  keywords: string,
  opts: Partial<LangDef> = {}
): LangDef => ({
  id,
  label,
  keywords: new Set(keywords.split(" ").filter(Boolean)),
  line: ["//"],
  block: [["/*", "*/"]],
  quotes: ['"', "'", "`"],
  ...opts,
});

const LANGS: LangDef[] = [
  def("json", "JSON", "true false null", {
    line: [],
    block: [],
    quotes: ['"'],
  }),
  def(
    "ts",
    "JavaScript / TypeScript",
    "as async await break case catch class const continue default delete do else enum export extends false finally for from function if implements import in instanceof interface let new null of return static super switch this throw true try type typeof undefined var void while yield"
  ),
  def(
    "rust",
    "Rust",
    "as async await break const continue crate dyn else enum extern false fn for if impl in let loop match mod move mut pub ref return self Self static struct super trait true type unsafe use where while"
  ),
  def(
    "python",
    "Python",
    "and as assert async await break class continue def del elif else except False finally for from global if import in is lambda None nonlocal not or pass raise return True try while with yield",
    { line: ["#"], block: [], quotes: ['"', "'"] }
  ),
  def(
    "sql",
    "SQL",
    "select from where insert into values update set delete create table alter drop index join left right inner outer on group by order having limit offset as and or not null distinct union all primary key foreign references default",
    { line: ["--"], block: [["/*", "*/"]], quotes: ["'", '"'] }
  ),
  def(
    "shell",
    "Shell",
    "if then else elif fi for while do done case esac function return export local echo cd exit set unset source",
    { line: ["#"], block: [], quotes: ['"', "'"] }
  ),
  def("css", "CSS", "important media import supports keyframes from to", {
    line: [],
    block: [["/*", "*/"]],
    quotes: ['"', "'"],
  }),
  def("xml", "HTML / XML", "", {
    line: [],
    block: [["<!--", "-->"]],
    quotes: ['"', "'"],
    markup: true,
  }),
];

const BY_ID = new Map(LANGS.map((l) => [l.id, l]));

/** Auswahlliste für den Umschalter im Detail-Bereich. */
export const LANGUAGES: Language[] = LANGS.map(({ id, label }) => ({
  id,
  label,
}));

const IDENT_START = /[A-Za-z_$@#-]/;
const IDENT_PART = /[\w$-]/;
const DIGIT = /[0-9]/;
const NUMBER_PART = /[\w.]/;

const ESCAPES: Record<string, string> = {
  "&": "&amp;",
  "<": "&lt;",
  ">": "&gt;",
  '"': "&quot;",
};
const ESCAPE_RE = /[&<>"]/g;

const escapeHtml = (s: string) => s.replace(ESCAPE_RE, (c) => ESCAPES[c]);

const span = (cls: string, text: string) =>
  `<span class="tok-${cls}">${escapeHtml(text)}</span>`;

// --- Erkennung --------------------------------------------------------------

const JSON_RE = /^[\s]*[[{][\s\S]*[\]}][\s]*$/;
const XML_RE = /^\s*<[a-zA-Z!/?]/;
const CSS_RE = /[.#]?[\w-]+\s*\{[^}]*:[^}]*(;|\})/;
const SHEBANG_RE = /^#!/;
const RUST_RE = /\b(fn|let\s+mut|impl|pub\s+fn|use\s+[\w:]+;|-> )/;
const PY_RE = /^\s*(def |class |import |from .+ import )/m;
const TS_RE = /\b(function|const|let|=>|export |import .+ from )/;
const SQL_RE = /\b(select .+ from|insert into|update .+ set|create table)\b/i;
const SHELL_RE = /^\s*(sudo |apt |npm |bun |git |cd |echo |curl |ls )/m;
/** Zeilen, die auf Code hindeuten: Einrückung oder ein Zeilenende, das in
    Fließtext praktisch nie vorkommt. */
const CODE_LINE_RE = /^\s{2,}\S|[;{}([\])]\s*$/;

/**
 * Sieht der Text insgesamt nach Code aus? Ein einzelnes Stichwort reicht nicht
 * — „->" oder „fn" tauchen auch in Notizen auf, und ein fälschlich eingefärbter
 * Fließtext ist schlimmer als gar keine Hervorhebung. Verlangt wird deshalb,
 * dass ein nennenswerter Anteil der Zeilen wie Code gebaut ist.
 */
function looksLikeCode(text: string): boolean {
  const lines = text.split("\n").filter((l) => l.trim() !== "");
  if (lines.length === 0) {
    return false;
  }
  const codeLines = lines.filter((l) => CODE_LINE_RE.test(l)).length;
  // Einzeiler (Befehl, Snippet) haben keine Struktur, die man zählen könnte.
  return lines.length === 1 ? true : codeLines / lines.length >= 0.3;
}

/**
 * Sprache raten. Ein Schnipsel hat keine Dateiendung, deshalb ist das eine
 * Heuristik und ausdrücklich fehlbar — die Vorschau lässt sie deshalb
 * umschalten. `null` = sieht nicht nach Code aus.
 */
export function detectLanguage(text: string): string | null {
  const t = text.trim();
  if (t.length < 12 || t.length > HIGHLIGHT_MAX_CHARS) {
    return null;
  }
  if (JSON_RE.test(t)) {
    try {
      JSON.parse(t);
      return "json";
    } catch {
      // Kein gültiges JSON — weiter mit den übrigen Mustern.
    }
  }
  if (XML_RE.test(t)) {
    return "xml";
  }
  if (SHEBANG_RE.test(t) || SHELL_RE.test(t)) {
    return "shell";
  }
  if (CSS_RE.test(t) && !TS_RE.test(t)) {
    return "css";
  }
  // Ab hier entscheiden einzelne Stichwörter — die greifen auch in Fließtext,
  // deshalb muss der Text zusätzlich strukturell nach Code aussehen.
  if (!looksLikeCode(t)) {
    return null;
  }
  if (SQL_RE.test(t)) {
    return "sql";
  }
  if (RUST_RE.test(t)) {
    return "rust";
  }
  if (PY_RE.test(t)) {
    return "python";
  }
  if (TS_RE.test(t)) {
    return "ts";
  }
  return null;
}

// --- Tokenisierung ----------------------------------------------------------

function startsWith(text: string, i: number, needle: string): boolean {
  return text.startsWith(needle, i);
}

function readString(text: string, start: number, quote: string): number {
  let i = start + 1;
  while (i < text.length) {
    if (text[i] === "\\") {
      i += 2;
      continue;
    }
    if (text[i] === quote) {
      return i + 1;
    }
    i += 1;
  }
  return text.length;
}

/** Ein Token ab Position `i`; `null` = hier beginnt keins dieser Art. */
type Token = { html: string; next: number } | null;

function readComment(text: string, i: number, lang: LangDef): Token {
  if (lang.line.some((prefix) => startsWith(text, i, prefix))) {
    const nl = text.indexOf("\n", i);
    const stop = nl === -1 ? text.length : nl;
    return { html: span("comment", text.slice(i, stop)), next: stop };
  }
  const block = lang.block.find(([open]) => startsWith(text, i, open));
  if (block) {
    const end = text.indexOf(block[1], i + block[0].length);
    const stop = end === -1 ? text.length : end + block[1].length;
    return { html: span("comment", text.slice(i, stop)), next: stop };
  }
  return null;
}

function readNumber(text: string, i: number): Token {
  if (!DIGIT.test(text[i])) {
    return null;
  }
  let j = i;
  while (j < text.length && NUMBER_PART.test(text[j])) {
    j += 1;
  }
  return { html: span("number", text.slice(i, j)), next: j };
}

function readWord(text: string, i: number, lang: LangDef): Token {
  if (!IDENT_START.test(text[i])) {
    return null;
  }
  // Ab i + 1: das Startzeichen gehört immer dazu. Sonst stünde bei „@" oder „#"
  // (Start-, aber kein Fortsetzungszeichen) j === i und die Schleife in
  // `highlight` käme nie von der Stelle.
  let j = i + 1;
  while (j < text.length && IDENT_PART.test(text[j])) {
    j += 1;
  }
  const word = text.slice(i, j);
  if (lang.keywords.has(word) || lang.keywords.has(word.toLowerCase())) {
    return { html: span("keyword", word), next: j };
  }
  // Bezeichner direkt vor einer Klammer liest sich als Aufruf.
  if (text[j] === "(") {
    return { html: span("fn", word), next: j };
  }
  return { html: escapeHtml(word), next: j };
}

/**
 * Text als HTML mit `tok-*`-Spans zurückgeben. Unbekannte Sprache oder zu
 * langer Text → nur escaped, ohne Farben.
 */
export function highlight(text: string, langId: string | null): string {
  if (!langId || text.length > HIGHLIGHT_MAX_CHARS) {
    return escapeHtml(text);
  }
  const lang = BY_ID.get(langId);
  if (!lang) {
    return escapeHtml(text);
  }
  if (lang.markup) {
    return highlightXml(text);
  }

  let out = "";
  let i = 0;
  while (i < text.length) {
    const ch = text[i];
    const quoted: Token = lang.quotes.includes(ch)
      ? (() => {
          const stop = readString(text, i, ch);
          return { html: span("string", text.slice(i, stop)), next: stop };
        })()
      : null;
    const token =
      readComment(text, i, lang) ??
      quoted ??
      readNumber(text, i) ??
      readWord(text, i, lang);
    if (token) {
      out += token.html;
      i = token.next;
    } else {
      out += escapeHtml(ch);
      i += 1;
    }
  }
  return out;
}

/** Markup: Tags, Attributnamen und Attributwerte getrennt einfärben. */
function highlightXml(text: string): string {
  let out = "";
  let i = 0;
  while (i < text.length) {
    if (startsWith(text, i, "<!--")) {
      const end = text.indexOf("-->", i);
      const stop = end === -1 ? text.length : end + 3;
      out += span("comment", text.slice(i, stop));
      i = stop;
      continue;
    }
    if (text[i] === "<") {
      const end = text.indexOf(">", i);
      const stop = end === -1 ? text.length : end + 1;
      out += highlightTag(text.slice(i, stop));
      i = stop;
      continue;
    }
    const next = text.indexOf("<", i);
    const stop = next === -1 ? text.length : next;
    out += escapeHtml(text.slice(i, stop));
    i = stop;
  }
  return out;
}

const TAG_PART_RE = /("[^"]*"|'[^']*')|([\w:-]+)/g;

function highlightTag(tag: string): string {
  let out = "";
  let last = 0;
  let seenName = false;
  TAG_PART_RE.lastIndex = 0;
  let m = TAG_PART_RE.exec(tag);
  while (m !== null) {
    out += span("tag", tag.slice(last, m.index));
    if (m[1]) {
      out += span("string", m[1]);
    } else if (seenName) {
      out += span("attr", m[2]);
    } else {
      out += span("tag-name", m[2]);
      seenName = true;
    }
    last = m.index + m[0].length;
    m = TAG_PART_RE.exec(tag);
  }
  out += span("tag", tag.slice(last));
  return out;
}
