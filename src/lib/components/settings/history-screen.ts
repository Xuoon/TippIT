// Auswahl „Historie öffnen auf": HistoryScreen ↔ Select-Wert und Optionsliste.
import type { HistoryScreen, MonitorInfo, WindowPosition } from "../../api";

export interface ScreenOption {
  label: string;
  value: string;
}

const MONITOR_PREFIX = "monitor:";
/** Select-Wert der verschobenen Position; sie ist kein `HistoryScreen`. */
export const REMEMBERED = "remembered";

/** Aktueller Select-Wert: eine verschobene Position hat Vorrang. */
export function placementKey(
  screen: HistoryScreen,
  position: WindowPosition | null
): string {
  return position ? REMEMBERED : screenKey(screen);
}

export function screenKey(screen: HistoryScreen): string {
  return screen.kind === "monitor"
    ? `${MONITOR_PREFIX}${screen.name}`
    : screen.kind;
}

export function screenFromKey(key: string): HistoryScreen {
  if (key.startsWith(MONITOR_PREFIX)) {
    return { kind: "monitor", name: key.slice(MONITOR_PREFIX.length) };
  }
  return key === "primary" ? { kind: "primary" } : { kind: "cursor" };
}

/** Optionen samt angeschlossener Monitore. Ein gespeicherter, gerade nicht
    angeschlossener Monitor bleibt sichtbar, sonst sähe die Auswahl leer aus. */
export function screenOptions(
  monitors: MonitorInfo[],
  current: HistoryScreen,
  position: WindowPosition | null = null
): ScreenOption[] {
  const options: ScreenOption[] = [
    ...(position
      ? [{ label: "Wo zuletzt verschoben", value: REMEMBERED }]
      : []),
    { label: "Monitor mit Mauszeiger", value: "cursor" },
    { label: "Hauptmonitor", value: "primary" },
  ];
  for (const m of monitors) {
    const value = `${MONITOR_PREFIX}${m.id}`;
    // Der Select braucht eindeutige Werte, auch falls das Backend eine
    // Kennung doppelt meldet.
    if (options.some((o) => o.value === value)) {
      continue;
    }
    const main = m.primary ? " · Hauptmonitor" : "";
    options.push({
      label: `${m.label} (${m.width} × ${m.height})${main}`,
      value,
    });
  }
  const key = screenKey(current);
  if (current.kind === "monitor" && !options.some((o) => o.value === key)) {
    // Die Kennung ist kein lesbarer Name.
    options.push({ label: "Gewählter Monitor (nicht verbunden)", value: key });
  }
  return options;
}
