// Messpunkte der Historie (Öffnen, Suche). Aktiv im Dev-Build oder mit
// localStorage "tippit.perf" = "1"; die Werte landen in der Konsole der WebView.

function enabled(): boolean {
  if (import.meta.env.DEV) {
    return true;
  }
  try {
    return localStorage.getItem("tippit.perf") === "1";
  } catch {
    return false;
  }
}

const ENABLED = enabled();
const started = new Map<string, number>();

/** Startet eine Messung; ein erneuter Start ersetzt den alten Zeitpunkt. */
export function perfStart(label: string): void {
  if (ENABLED) {
    started.set(label, performance.now());
  }
}

/** Beendet laufende Messungen nach dem nächsten gemalten Frame. */
export function perfFrame(...labels: string[]): void {
  if (!ENABLED) {
    return;
  }
  const open = labels.filter((l) => started.has(l));
  if (open.length === 0) {
    return;
  }
  requestAnimationFrame(() => {
    const now = performance.now();
    for (const label of open) {
      const t0 = started.get(label);
      if (t0 !== undefined) {
        started.delete(label);
        console.debug(`[perf] ${label}: ${(now - t0).toFixed(1)} ms`);
      }
    }
  });
}
