import { expect, test } from "bun:test";
import { hotkeyFromEvent, sameHotkey } from "./hotkey-capture";

const press = (
  key: string,
  code: string,
  mods: Partial<Record<"altKey" | "ctrlKey" | "metaKey" | "shiftKey", boolean>>
) => ({
  altKey: false,
  code,
  ctrlKey: false,
  key,
  metaKey: false,
  shiftKey: false,
  ...mods,
});

test("Modifier in fester Reihenfolge", () => {
  expect(
    hotkeyFromEvent(press("E", "KeyE", { shiftKey: true, ctrlKey: true }))
  ).toBe("ctrl+shift+e");
  expect(hotkeyFromEvent(press("e", "KeyE", { metaKey: true }))).toBe(
    "super+e"
  );
});

test("layoutbewusst: QWERTZ-Z kommt aus event.key, nicht aus dem US-Code", () => {
  expect(hotkeyFromEvent(press("z", "KeyY", { ctrlKey: true }))).toBe("ctrl+z");
});

test("Shift+Ziffer fällt auf den physischen Code zurück", () => {
  expect(
    hotkeyFromEvent(press("!", "Digit1", { ctrlKey: true, shiftKey: true }))
  ).toBe("ctrl+shift+1");
});

test("F-Tasten werden angenommen", () => {
  expect(hotkeyFromEvent(press("F12", "F12", { altKey: true }))).toBe(
    "alt+f12"
  );
});

test("ohne Modifier oder mit nicht unterstützter Taste kein Hotkey", () => {
  expect(hotkeyFromEvent(press("e", "KeyE", {}))).toBeNull();
  expect(
    hotkeyFromEvent(press("Control", "ControlLeft", { ctrlKey: true }))
  ).toBeNull();
  expect(
    hotkeyFromEvent(press("Enter", "Enter", { ctrlKey: true }))
  ).toBeNull();
});

test("sameHotkey vergleicht unabhängig von Schreibweise und Reihenfolge", () => {
  expect(sameHotkey("cmd+shift+e", "shift+super+e")).toBe(true);
  expect(sameHotkey("CTRL+E", "ctrl+e")).toBe(true);
  expect(sameHotkey("ctrl+e", "ctrl+shift+e")).toBe(false);
});
