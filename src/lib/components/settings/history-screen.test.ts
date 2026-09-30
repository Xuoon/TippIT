import { describe, expect, test } from "bun:test";
import type { MonitorInfo } from "../../api";
import {
  placementKey,
  REMEMBERED,
  screenFromKey,
  screenKey,
  screenOptions,
} from "./history-screen";

const MONITORS: MonitorInfo[] = [
  {
    height: 1117,
    id: "610-a050-0",
    label: "Built-in Display",
    primary: true,
    width: 1728,
  },
  {
    height: 1440,
    id: "10ac-a0c4-0",
    label: "DELL U2720Q",
    primary: false,
    width: 2560,
  },
];

describe("screenKey/screenFromKey", () => {
  test("Rundlauf für alle Varianten", () => {
    for (const screen of [
      { kind: "cursor" },
      { kind: "primary" },
      { kind: "monitor", name: "DELL: links" },
    ] as const) {
      expect(screenFromKey(screenKey(screen))).toEqual(screen);
    }
  });

  test("unbekannter Wert fällt auf den Mauszeiger zurück", () => {
    expect(screenFromKey("egal")).toEqual({ kind: "cursor" });
  });
});

describe("screenOptions", () => {
  test("Standardoptionen, dann Monitore mit Auflösung und Hauptmonitor", () => {
    expect(screenOptions(MONITORS, { kind: "cursor" })).toEqual([
      { label: "Monitor mit Mauszeiger", value: "cursor" },
      { label: "Hauptmonitor", value: "primary" },
      {
        label: "Built-in Display (1728 × 1117) · Hauptmonitor",
        value: "monitor:610-a050-0",
      },
      { label: "DELL U2720Q (2560 × 1440)", value: "monitor:10ac-a0c4-0" },
    ]);
  });

  test("gespeicherter, getrennter Monitor bleibt als Eintrag", () => {
    const options = screenOptions(MONITORS, {
      kind: "monitor",
      name: "4c2d-1234-0",
    });
    expect(options.at(-1)).toEqual({
      label: "Gewählter Monitor (nicht verbunden)",
      value: "monitor:4c2d-1234-0",
    });
  });

  test("gleiche Modelle mit eigener Kennung ergeben zwei Optionen", () => {
    const twins = [
      { ...MONITORS[1], id: "10ac-a0c4-0@0,0", label: "DELL U2720Q (1)" },
      { ...MONITORS[1], id: "10ac-a0c4-0@1440,0", label: "DELL U2720Q (2)" },
    ];
    const values = screenOptions(twins, { kind: "cursor" }).map((o) => o.value);
    expect(values).toEqual([
      "cursor",
      "primary",
      "monitor:10ac-a0c4-0@0,0",
      "monitor:10ac-a0c4-0@1440,0",
    ]);
  });

  test("doppelt gemeldete Kennung ergibt nur eine Option", () => {
    const values = screenOptions([MONITORS[1], { ...MONITORS[1] }], {
      kind: "cursor",
    }).map((o) => o.value);
    expect(new Set(values).size).toBe(values.length);
  });
});

describe("verschobene Position", () => {
  const position = { monitor: "10ac-a0c4-0", x: 0.25, y: 1 };

  test("hat Vorrang vor der Regel", () => {
    expect(placementKey({ kind: "primary" }, position)).toBe(REMEMBERED);
    expect(placementKey({ kind: "primary" }, null)).toBe("primary");
  });

  test("steht nur mit Position als erste Option", () => {
    expect(screenOptions(MONITORS, { kind: "cursor" }, position)[0]).toEqual({
      label: "Wo zuletzt verschoben",
      value: REMEMBERED,
    });
    const values = screenOptions(MONITORS, { kind: "cursor" }).map(
      (o) => o.value
    );
    expect(values).not.toContain(REMEMBERED);
  });
});
