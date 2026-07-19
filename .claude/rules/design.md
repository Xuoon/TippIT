---
paths:
  - "src/**"
---

# Design & Frontend-Konventionen

- Farben/Radien/Typo NUR über `var(--…)` aus `src/lib/theme.css` — nie rohe Hex-Werte im Komponenten-CSS. Neue Tokens immer in BEIDEN Blöcken pflegen (`:root` = dunkel, `:root[data-theme="light"]` = hell).
- Theme-Anwendung: jede Route ruft in `onMount` `initTheme()` aus `src/lib/theme.ts` auf (setzt `data-theme` auf `<html>`, folgt live `settings-changed` + System-Schema) und gibt dessen Cleanup zurück.
- Neue Eintrags-Typen/Filter der Historie NUR in `src/lib/entry-kinds.ts` registrieren (Label, Icon, `--kind-*`/`--kind-*-tint`-Token, Backend-kind, optionale `refine`-Funktion) — die Historie-UI liest ausschließlich diese Registry.
- Icons: Inline-SVG-Set in `src/lib/icon.svelte` (viewBox 16×16, `stroke=currentColor`, keine Emoji); neue Dateien unter `src/lib` kebab-case (Ultracite-Regel).
