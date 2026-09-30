import { describe, expect, test } from "bun:test";
import { fmtBytes, formatCode } from "./format";

describe("fmtBytes", () => {
  test("Einheitengrenzen", () => {
    expect(fmtBytes(0)).toBe("0 B");
    expect(fmtBytes(1023)).toBe("1023 B");
    expect(fmtBytes(1024)).toBe("1.0 KB");
    expect(fmtBytes(1024 * 1024)).toBe("1.0 MB");
  });
});

describe("formatCode", () => {
  test("gruppiert 6- und 8-stellige Codes mittig", () => {
    expect(formatCode("287082")).toBe("287 082");
    expect(formatCode("46119246")).toBe("4611 9246");
  });
});
