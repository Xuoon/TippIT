// biome-ignore-all lint/suspicious/noBitwiseOperators: TOTP ist per RFC 4226/4648 Bit-Arithmetik
// TOTP (RFC 6238) clientseitig über Web Crypto (HMAC).
// Passt zur Registry-Regel „TOTP ist eine Text-Verfeinerung" (entry-kinds.ts):
// aus dem entschlüsselten Eintragstext (otpauth-URI oder rohes Base32-Secret)
// wird live der Code erzeugt — nichts wird persistiert.

const BASE32_ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
const OTPAUTH_RE = /^otpauth:\/\//i;
const PAD_OR_SPACE_RE = /[=\s]/g;

/** Base32 (RFC 4648, ohne Padding) → Bytes; unbekannte Zeichen werden ignoriert. */
function base32Decode(input: string): Uint8Array {
  const clean = input.replace(PAD_OR_SPACE_RE, "").toUpperCase();
  let bits = 0;
  let value = 0;
  const out: number[] = [];
  for (const ch of clean) {
    const idx = BASE32_ALPHABET.indexOf(ch);
    if (idx === -1) {
      continue;
    }
    value = (value << 5) | idx;
    bits += 5;
    if (bits >= 8) {
      bits -= 8;
      out.push((value >>> bits) & 0xff);
    }
  }
  return new Uint8Array(out);
}

type TotpAlgorithm = "SHA-1" | "SHA-256" | "SHA-512";

function normalizeAlgorithm(raw: string | null): TotpAlgorithm {
  const alg = (raw ?? "SHA1").toUpperCase();
  if (alg === "SHA256") {
    return "SHA-256";
  }
  if (alg === "SHA512") {
    return "SHA-512";
  }
  return "SHA-1";
}

export interface TotpConfig {
  algorithm: TotpAlgorithm;
  digits: number;
  period: number;
  secret: Uint8Array;
}

/** otpauth://-URI oder rohes Base32-Secret parsen (Defaults: 6 Stellen, 30 s, SHA-1). */
export function parseTotp(raw: string): TotpConfig | null {
  const text = raw.trim();
  if (OTPAUTH_RE.test(text)) {
    let url: URL;
    try {
      url = new URL(text);
    } catch {
      return null;
    }
    const secret = url.searchParams.get("secret");
    if (!secret) {
      return null;
    }
    const bytes = base32Decode(secret);
    if (bytes.length === 0) {
      return null;
    }
    return {
      algorithm: normalizeAlgorithm(url.searchParams.get("algorithm")),
      digits: Number(url.searchParams.get("digits")) || 6,
      period: Number(url.searchParams.get("period")) || 30,
      secret: bytes,
    };
  }
  const bytes = base32Decode(text);
  if (bytes.length === 0) {
    return null;
  }
  return { algorithm: "SHA-1", digits: 6, period: 30, secret: bytes };
}

async function hotp(cfg: TotpConfig, counter: number): Promise<string> {
  // 8-Byte-Counter, Big-Endian. JS-Bitshift ist 32-bit → High-Word separat
  // (der klassische `counter >> 32`-Bug).
  const buf = new ArrayBuffer(8);
  const view = new DataView(buf);
  view.setUint32(0, Math.floor(counter / 2 ** 32));
  view.setUint32(4, counter >>> 0);
  const key = await crypto.subtle.importKey(
    "raw",
    cfg.secret,
    { name: "HMAC", hash: cfg.algorithm },
    false,
    ["sign"]
  );
  const sig = new Uint8Array(await crypto.subtle.sign("HMAC", key, buf));
  // RFC 4226 Dynamic Truncation.
  const offset = (sig.at(-1) ?? 0) & 0x0f;
  const bin =
    ((sig[offset] & 0x7f) << 24) |
    ((sig[offset + 1] & 0xff) << 16) |
    ((sig[offset + 2] & 0xff) << 8) |
    (sig[offset + 3] & 0xff);
  return (bin % 10 ** cfg.digits).toString().padStart(cfg.digits, "0");
}

export interface TotpNow {
  /** Aktueller Code, auf `digits` Stellen aufgefüllt. */
  code: string;
  /** Periodenlänge in Sekunden (i. d. R. 30). */
  period: number;
  /** Sekunden bis der Code wechselt. */
  remaining: number;
}

/** Aktuellen Code + Restlaufzeit berechnen. `nowMs` injizierbar für Tests. */
export async function totpNow(
  cfg: TotpConfig,
  nowMs: number = Date.now()
): Promise<TotpNow> {
  const epoch = Math.floor(nowMs / 1000);
  const counter = Math.floor(epoch / cfg.period);
  const code = await hotp(cfg, counter);
  return {
    code,
    period: cfg.period,
    remaining: cfg.period - (epoch % cfg.period),
  };
}
