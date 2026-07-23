---
paths:
  - "src/**"
---

# Design & Frontend-Konventionen

- Farben/Typo/Radien NUR über `var(--…)` aus `src/lib/theme.css` — nie rohe Hex-Werte im Komponenten-CSS.
- **Strukturelle Tokens** (Radien `--r-*`, Density `--row-h`/`--rail-w`/…, Typo, Timing) nur in `:root`. **Theme-Farben/Shadows** in beiden Blöcken (`:root` + `:root[data-theme="light"]`).
- Theme-Anwendung: jede Route ruft in `onMount` `initTheme()` aus `src/lib/theme.ts` auf und gibt dessen Cleanup zurück.
- History/Update: `html`/`body` transparent nur **route-scoped** (CSS `:global`); Settings nie transparent machen (`app.html` bleibt opaker Fallback).
- Neue Eintrags-Typen/Filter/Primäraktionen (`primaryAction`) der Historie NUR in `src/lib/entry-kinds.ts` — die Historie-UI liest ausschließlich diese Registry.
- Icons: Inline-SVG in `src/lib/icon.svelte` (viewBox 16×16, `stroke=currentColor`); Dateien unter `src/lib` kebab-case.
