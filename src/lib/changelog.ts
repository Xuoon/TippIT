// Minimal-Parser für CHANGELOG.md im Keep-a-Changelog-Format
// (## Release, ### Gruppe, - Punkt); die Einstellungen bündeln die Datei per ?raw.

export interface LogGroup {
  items: string[];
  title: string;
}

export interface LogRelease {
  groups: LogGroup[];
  intro: string[];
  title: string;
}

/** Markdown-Inline-Reste entfernen (Links → Text, ** und ` weg). */
function cleanMd(s: string): string {
  return s
    .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
    .replaceAll("**", "")
    .replaceAll("`", "");
}

export function parseChangelog(md: string): LogRelease[] {
  const releases: LogRelease[] = [];
  let release: LogRelease | null = null;
  let group: LogGroup | null = null;
  for (const raw of md.split("\n")) {
    const line = raw.trimEnd();
    if (line.startsWith("## ")) {
      release = {
        groups: [],
        intro: [],
        title: cleanMd(line.slice(3)).replace("[", "").replace("]", ""),
      };
      releases.push(release);
      group = null;
    } else if (line.startsWith("### ") && release) {
      group = { items: [], title: line.slice(4) };
      release.groups.push(group);
    } else if (line.startsWith("- ")) {
      group?.items.push(cleanMd(line.slice(2)));
    } else if (line !== "" && release && !group && !line.startsWith("#")) {
      release.intro.push(cleanMd(line));
    }
  }
  return releases;
}

/** Release-Titel „2.0.0 – 2026-08-19" in Version und Datum trennen. */
export function releaseName(title: string): string {
  const name = title.split(" – ")[0].trim();
  return name.startsWith("Unreleased") ? "Unveröffentlicht" : name;
}

export function releaseDate(title: string): string {
  return title.split(" – ")[1]?.trim() ?? "";
}

/** Farbschlüssel je Gruppe: Neues grün, Geändertes blau, Entferntes rot,
    Behobenes gelb. Unbekannte Überschriften bleiben neutral. */
const GROUP_TONES: Record<string, string> = {
  Hinzugefügt: "add",
  Geändert: "change",
  Entfernt: "remove",
  Behoben: "fix",
};

export const groupTone = (title: string) => GROUP_TONES[title.trim()] ?? "";
