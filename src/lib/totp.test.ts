import { describe, expect, test } from "bun:test";
import { maskOtpauthSecret, parseTotp, type TotpConfig, totpNow } from "./totp";

// RFC 6238, Anhang B: Schlüssel je Algorithmus, 8 Stellen, 30 s.
const KEYS = {
  "SHA-1": "12345678901234567890",
  "SHA-256": "12345678901234567890123456789012",
  "SHA-512": "1234567890123456789012345678901234567890123456789012345678901234",
} as const;

const VECTORS: [number, string, string, string][] = [
  [59, "94287082", "46119246", "90693936"],
  [1_111_111_109, "07081804", "68084774", "25091201"],
  [1_111_111_111, "14050471", "67062674", "99943326"],
  [1_234_567_890, "89005924", "91819424", "93441116"],
  [2_000_000_000, "69279037", "90698825", "38618901"],
  [20_000_000_000, "65353130", "77737706", "47863826"],
];

function config(algorithm: TotpConfig["algorithm"]): TotpConfig {
  return {
    algorithm,
    digits: 8,
    period: 30,
    secret: new Uint8Array(new TextEncoder().encode(KEYS[algorithm])),
  };
}

describe("totpNow", () => {
  for (const [seconds, sha1, sha256, sha512] of VECTORS) {
    test(`RFC-6238-Vektor bei ${seconds} s`, async () => {
      const nowMs = seconds * 1000;
      expect((await totpNow(config("SHA-1"), nowMs)).code).toBe(sha1);
      expect((await totpNow(config("SHA-256"), nowMs)).code).toBe(sha256);
      expect((await totpNow(config("SHA-512"), nowMs)).code).toBe(sha512);
    });
  }

  test("Restlaufzeit innerhalb der Periode", async () => {
    const now = await totpNow(config("SHA-1"), 59_000);
    expect(now.period).toBe(30);
    expect(now.remaining).toBe(1);
  });
});

describe("parseTotp", () => {
  // Base32 von "12345678901234567890".
  const SECRET = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";

  test("rohes Base32-Secret mit Defaults", async () => {
    const cfg = parseTotp(SECRET);
    expect(cfg).not.toBeNull();
    expect(cfg?.algorithm).toBe("SHA-1");
    expect(cfg?.digits).toBe(6);
    expect(cfg?.period).toBe(30);
    // Letzte 6 Stellen des 8-stelligen RFC-Werts.
    if (cfg) {
      expect((await totpNow(cfg, 59_000)).code).toBe("287082");
    }
  });

  test("otpauth-URI mit Parametern", () => {
    const cfg = parseTotp(
      `otpauth://totp/Beispiel:max?secret=${SECRET}&algorithm=SHA256&digits=8&period=60`
    );
    expect(cfg?.algorithm).toBe("SHA-256");
    expect(cfg?.digits).toBe(8);
    expect(cfg?.period).toBe(60);
    expect(cfg && new TextDecoder().decode(cfg.secret)).toBe(
      "12345678901234567890"
    );
  });

  test("Kleinbuchstaben, Leerzeichen und Padding im Secret", () => {
    const cfg = parseTotp("gezd gnbv gy3t qojq gezd gnbv gy3t qojq====");
    expect(cfg && new TextDecoder().decode(cfg.secret)).toBe(
      "12345678901234567890"
    );
  });

  test("otpauth ohne Secret oder kaputte URI", () => {
    expect(parseTotp("otpauth://totp/x?issuer=y")).toBeNull();
    expect(parseTotp("otpauth://totp/x?secret=")).toBeNull();
  });

  test("leerer Text", () => {
    expect(parseTotp("   ")).toBeNull();
  });
});

describe("maskOtpauthSecret", () => {
  test("verdeckt den secret-Parameter", () => {
    expect(maskOtpauthSecret("otpauth://totp/A:b?secret=ABCDEF&issuer=A")).toBe(
      "otpauth://totp/A:b?secret=••••&issuer=A"
    );
    expect(maskOtpauthSecret("otpauth://totp/A?issuer=A&SECRET=XYZ")).toBe(
      "otpauth://totp/A?issuer=A&SECRET=••••"
    );
  });

  test("lässt rohe Secrets und anderen Text unverändert", () => {
    expect(maskOtpauthSecret("JBSWY3DPEHPK3PXP")).toBe("JBSWY3DPEHPK3PXP");
    expect(maskOtpauthSecret("https://x.de/?secret=1")).toBe(
      "https://x.de/?secret=1"
    );
  });
});
