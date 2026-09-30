// Zahlenwert der Stepper-Zeilen: Schritte rasten auf das Raster ein.

export function clampValue(value: number, min: number, max: number): number {
  if (!Number.isFinite(value)) {
    return min;
  }
  return Math.min(max, Math.max(min, Math.round(value)));
}

/** Nächster Rasterwert in Richtung `dir`; ein krummer Wert rastet zuerst ein
    (17 → 20 bzw. 15 bei Schritt 5). */
export function stepValue(
  value: number,
  dir: 1 | -1,
  step: number,
  min: number,
  max: number
): number {
  const next =
    dir > 0
      ? Math.floor(value / step) * step + step
      : Math.ceil(value / step) * step - step;
  return clampValue(next, min, max);
}
