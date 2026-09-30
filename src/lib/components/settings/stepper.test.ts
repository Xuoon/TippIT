import { expect, test } from "bun:test";
import { clampValue, stepValue } from "./stepper";

test("Schritt auf dem Raster", () => {
  expect(stepValue(15, 1, 5, 1, 100)).toBe(20);
  expect(stepValue(15, -1, 5, 1, 100)).toBe(10);
});

test("krummer Wert rastet ein", () => {
  expect(stepValue(17, 1, 5, 1, 100)).toBe(20);
  expect(stepValue(17, -1, 5, 1, 100)).toBe(15);
});

test("Grenzen halten", () => {
  expect(stepValue(5, -1, 5, 1, 100)).toBe(1);
  expect(stepValue(1, -1, 5, 1, 100)).toBe(1);
  expect(stepValue(100, 1, 5, 1, 100)).toBe(100);
  expect(stepValue(4900, 1, 100, 100, 5000)).toBe(5000);
});

test("Eingabe wird begrenzt und gerundet", () => {
  expect(clampValue(12.6, 1, 100)).toBe(13);
  expect(clampValue(0, 1, 100)).toBe(1);
  expect(clampValue(9999, 100, 5000)).toBe(5000);
  expect(clampValue(Number.NaN, 100, 5000)).toBe(100);
});
