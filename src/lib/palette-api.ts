// Commands der Mini-Palette (src-tauri/src/palette.rs).
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface PaletteEntry {
  has_thumb: boolean;
  kind: number;
  preview: string;
  snippet: boolean;
  uuid: string;
}

export const paletteEntries = () => invoke<PaletteEntry[]>("palette_entries");

/** Gerendert: Backend legt das Fenster in dieser Größe an den Mauszeiger. */
export const paletteReady = (width: number, height: number) =>
  invoke<void>("palette_ready", { height, width });

export const paletteHide = () => invoke<void>("palette_hide");

/** Einfügen bzw. mit `typeChars` zeichenweise tippen. Bei TOTP geht nur der
    live erzeugte `code` mit, nie das Secret. */
export const palettePick = (uuid: string, typeChars: boolean, code?: string) =>
  invoke<void>("palette_pick", { code, typeChars, uuid });

export const onPaletteOpen = (cb: () => void): Promise<UnlistenFn> =>
  listen("palette-open", () => cb());
