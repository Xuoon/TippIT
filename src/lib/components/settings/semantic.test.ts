import { describe, expect, test } from "bun:test";
import type { SemanticStatus } from "#lib/api.ts";
import { phasePercent, phaseText } from "./semantic.ts";

const base: SemanticStatus = {
  phase: "ready",
  download_bytes: 220_000_000,
  embeddable: 10,
  enabled: true,
  loaded: false,
  model_bytes: 0,
  model_ram_bytes: 310_000_000,
  outdated_model_bytes: 0,
  ram_bytes: 0,
  vectors: { bytes: 0, current: 0, stale: 0 },
};

describe("semantic", () => {
  test("Fortschritt nur beim Laden und Einlesen", () => {
    expect(phasePercent(base)).toBeNull();
    expect(
      phasePercent({ ...base, phase: "indexing", done: 1, total: 3 })
    ).toBe(33);
    expect(
      phasePercent({ ...base, phase: "downloading", done: 5, total: 0 })
    ).toBe(0);
  });

  test("Bereit unterscheidet geladenes und ruhendes Modell", () => {
    expect(phaseText(base)).toContain("ruht");
    expect(phaseText({ ...base, loaded: true })).toBe("Bereit");
    expect(phaseText({ ...base, phase: "error", message: "kein Netz" })).toBe(
      "Fehler: kein Netz"
    );
  });
});
