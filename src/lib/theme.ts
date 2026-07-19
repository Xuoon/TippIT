// Theme-Laufzeit: setzt <html data-theme="dark|light"> anhand der Einstellung
// „Darstellung" und hält das live aktuell — bei Einstellungs-Änderungen (auch
// aus anderen Fenstern, via settings-changed) und bei Wechseln des
// System-Schemas im Modus „system".
import { getSettings, onSettingsChanged } from "./api";

const media = window.matchMedia("(prefers-color-scheme: light)");
let mode = "system";

function apply() {
  const light = mode === "light" || (mode === "system" && media.matches);
  document.documentElement.dataset.theme = light ? "light" : "dark";
}

/** Sofort anwenden (z. B. direkt beim Ändern des Selects, ohne Save-Roundtrip). */
export function setThemeMode(next: string) {
  mode = next;
  apply();
}

/** In onMount jeder Route aufrufen; Rückgabe ist die Cleanup-Funktion. */
export function initTheme(): () => void {
  getSettings()
    .then((s) => setThemeMode(s.theme))
    .catch(() => {
      // Ohne Settings bleibt der Default (system/dark) stehen.
    });
  const unlisten = onSettingsChanged((s) => setThemeMode(s.theme));
  media.addEventListener("change", apply);
  return () => {
    media.removeEventListener("change", apply);
    unlisten.then((stop) => stop());
  };
}
