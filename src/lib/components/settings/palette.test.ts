import { expect, test } from "bun:test";
import {
  modifierChoices,
  modifierLabel,
  paletteCounts,
  paletteModifiers,
} from "./palette";

test("⌘ nur unter macOS", () => {
  expect(paletteModifiers(true)).toContain("cmd");
  expect(paletteModifiers(false)).not.toContain("cmd");
});

test("Beschriftung je Plattform", () => {
  expect(modifierLabel("alt", true)).toBe("⌥");
  expect(modifierLabel("shift", true)).toBe("⇧");
  expect(modifierLabel("ctrl", false)).toBe("STRG");
  expect(modifierLabel("alt", false)).toBe("ALT");
});

test("der andere Modifier ist gesperrt", () => {
  const choices = modifierChoices(false, "shift");
  expect(choices.filter((c) => c.disabled).map((c) => c.value)).toEqual([
    "shift",
  ]);
  expect(choices.map((c) => c.value)).toEqual(["alt", "ctrl", "shift"]);
});

test("Anzahl: Raster plus krummer Wert", () => {
  expect(paletteCounts(5)).toEqual([3, 5, 7, 9]);
  expect(paletteCounts(4)).toEqual([3, 4, 5, 7, 9]);
});
