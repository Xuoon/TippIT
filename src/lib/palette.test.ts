import { describe, expect, test } from "bun:test";
import { digitIndex, modifierHeld } from "./palette";

const none = { altKey: false, ctrlKey: false, metaKey: false, shiftKey: false };

describe("modifierHeld", () => {
  test("prüft genau den gewählten Modifier", () => {
    expect(modifierHeld({ ...none, shiftKey: true }, "shift")).toBe(true);
    expect(modifierHeld({ ...none, shiftKey: true }, "alt")).toBe(false);
    expect(modifierHeld({ ...none, metaKey: true }, "cmd")).toBe(true);
    expect(modifierHeld({ ...none, ctrlKey: true }, "ctrl")).toBe(true);
    expect(modifierHeld(none, "alt")).toBe(false);
  });
});

describe("digitIndex", () => {
  test("Ziffernreihe und Ziffernblock", () => {
    expect(digitIndex("Digit1", 5)).toBe(0);
    expect(digitIndex("Numpad5", 5)).toBe(4);
  });
  test("außerhalb der Einträge oder keine Ziffer", () => {
    expect(digitIndex("Digit6", 5)).toBeNull();
    expect(digitIndex("Digit0", 9)).toBeNull();
    expect(digitIndex("KeyA", 5)).toBeNull();
  });
});
