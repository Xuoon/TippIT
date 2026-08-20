import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface EntryDto {
  copy_count: number;
  created_at: number;
  first_created_at: number;
  /** Es liegt eine formatierte Fassung vor (Rich-Text-Einfügen möglich). */
  has_html: boolean;
  has_thumb: boolean;
  kind: number; // 0 Text, 1 Bild, 2 Dateien
  pinned: boolean;
  preview: string;
  size_bytes: number;
  /** Dauerhafter Textbaustein statt erfasster Kopie. */
  snippet: boolean;
  /** Bundle-ID / exe path — geräte-lokal, nullable. */
  source_app_id: string | null;
  source_app_name: string | null;
  uuid: string;
}

export interface TargetAppDto {
  id: string;
  name: string;
}

export interface Settings {
  history: {
    max_entries: number;
    capture_images: boolean;
    capture_files: boolean;
    /** Formatierung (Clipboard-HTML) sanitisiert mitspeichern. */
    capture_html: boolean;
    /** Einträge nach so vielen Tagen in den Papierkorb legen; 0 = aus. */
    retention_days: number;
    /** Quell-App-IDs, aus denen nichts erfasst wird. */
    excluded_apps: string[];
    /** Fenstergröße in Prozent der Basisgröße (100 = Standard). */
    window_scale: number;
  };
  /** Abbrechen des Tippens ist fest ESC (kein Setting, s. typing.rs). */
  hotkeys: { paste: string; history: string };
  sounds: boolean;
  theme: string;
  typing: {
    pre_delay_ms: number;
    mode: "bulk" | "per_char";
    char_delay_ms: number;
    trim: boolean;
  };
}

export const KIND_TEXT = 0;
export const KIND_IMAGE = 1;
export const KIND_FILES = 2;

export const searchHistory = (query: string, kind: number | null) =>
  invoke<EntryDto[]>("search_history", { query, kind });

export const entryThumb = (uuid: string) =>
  invoke<string | null>("entry_thumb", { uuid });

/** Volles Bild (data-URL) für die Detail-Vorschau — nur für den ausgewählten Eintrag. */
export const entryImage = (uuid: string) =>
  invoke<string | null>("entry_image", { uuid });

/** Quellanwendungs-Icon (data-URL) aus Disk-Cache. */
export const sourceAppIcon = (appId: string) =>
  invoke<string | null>("source_app_icon", { appId });

/** Ziel-App für „In … einfügen" (vor History-Öffnen). */
export const historyTargetApp = () =>
  invoke<TargetAppDto | null>("history_target_app");

/** Eine erkannte OCR-Textzeile mit normalisierter Position (Ursprung oben-links). */
export interface OcrBlock {
  h: number;
  text: string;
  w: number;
  x: number;
  y: number;
}

/** OCR-Ergebnis: Klartext (Panel/Kopieren) + positionierte Zeilen (Overlay). */
export interface OcrResult {
  blocks: OcrBlock[];
  text: string;
}

/** OCR: Text aus Bild-Eintrag (macOS Vision, Windows WinRT-OCR). */
export const ocrEntry = (uuid: string) =>
  invoke<OcrResult>("ocr_entry", { uuid });

/** QR-Codes eines Bild-Eintrags dekodieren (Inhalte aller lesbaren Codes). */
export const qrEntry = (uuid: string) => invoke<string[]>("qr_entry", { uuid });

/** http(s)-Link öffnen (z. B. dekodierter QR-Inhalt). */
export const openLink = (url: string) => invoke<void>("open_link", { url });

/** UI-Text kopieren und den eigenen Clipboard-Write im Monitor markieren. */
export const copyText = (text: string) => invoke<void>("copy_text", { text });

/** Wie der Inhalt für EINEN Vorgang ins Zielfenster kommt; ohne Angabe gilt der
    eingestellte Tippmodus. `paste` löst STRG+V (⌘V) aus und setzt voraus, dass
    der Inhalt vorher in die Zwischenablage gelegt wurde. */
export type WriteMode = "bulk" | "paste" | "per_char";

/** Beliebigen Text ins zuvor fokussierte Fenster tippen (z. B. den TOTP-Code). */
export const typeText = (text: string, mode?: WriteMode) =>
  invoke<void>("type_text", { mode, text });

/** Datenverzeichnis ~/.labi/tippit/ im Dateimanager öffnen. */
export const openDataDir = () => invoke<void>("open_data_dir");

/** Datei(en) bzw. http(s)-Link eines Eintrags im Standard-Handler öffnen. */
export const openEntry = (uuid: string) => invoke<void>("open_entry", { uuid });

export const entryText = (uuid: string) =>
  invoke<string | null>("entry_text", { uuid });

export const copyEntry = (uuid: string) => invoke<void>("copy_entry", { uuid });
export const typeEntry = (uuid: string, mode?: WriteMode) =>
  invoke<void>("type_entry", { mode, uuid });
export const pinEntry = (uuid: string, pinned: boolean) =>
  invoke<void>("pin_entry", { uuid, pinned });
/** Löschen legt in den Papierkorb — endgültig erst über purgeEntry/emptyTrash. */
export const deleteEntry = (uuid: string) =>
  invoke<void>("delete_entry", { uuid });
export const restoreEntry = (uuid: string) =>
  invoke<void>("restore_entry", { uuid });
export const purgeEntry = (uuid: string) =>
  invoke<void>("purge_entry", { uuid });
export const emptyTrash = () => invoke<number>("empty_trash");
export const clearHistory = () => invoke<void>("clear_history");

/** Ein Eintrag im Papierkorb (wird bei jedem Aufruf frisch entschlüsselt). */
export interface TrashDto {
  kind: number;
  preview: string;
  size_bytes: number;
  trashed_at: number;
  uuid: string;
}

export const listTrash = () => invoke<TrashDto[]>("list_trash");

/** Eintrag zum dauerhaften Textbaustein machen (oder zurück). */
export const setEntrySnippet = (uuid: string, snippet: boolean) =>
  invoke<void>("set_entry_snippet", { snippet, uuid });

/** Neuen Textbaustein anlegen; liefert dessen uuid. */
export const createSnippet = (text: string) =>
  invoke<string>("create_snippet", { text });

/** Sanitisiertes HTML eines Eintrags für die Vorschau (null = keine Formatierung). */
export const entryHtml = (uuid: string) =>
  invoke<string | null>("entry_html", { uuid });

/** Bild-Eintrag als PNG speichern; null = Dialog abgebrochen. */
export const saveEntryImage = (uuid: string) =>
  invoke<string | null>("save_entry_image", { uuid });

/** Verschlüsselte Sicherung schreiben; null = abgebrochen, sonst Anzahl Einträge. */
export const exportHistory = (password: string) =>
  invoke<number | null>("export_history", { password });

export interface ImportReport {
  imported: number;
  skipped: number;
}

/** Sicherung einlesen und zusammenführen; null = abgebrochen. */
export const importHistory = (password: string) =>
  invoke<ImportReport | null>("import_history", { password });
export const hideHistoryWindow = () => invoke<void>("hide_history_window");

export const getSettings = () => invoke<Settings>("get_settings");
export const setSettings = (settings: Settings) =>
  invoke<void>("set_settings", { settings });
/** Auslieferungs-Defaults (defaults.json + Rust-Defaults) — einzige Quelle. */
export const getDefaultSettings = () => invoke<Settings>("default_settings");

export const onHistoryChanged = (cb: () => void): Promise<UnlistenFn> =>
  listen("history-changed", cb);

export const onSettingsChanged = (
  cb: (s: Settings) => void
): Promise<UnlistenFn> =>
  listen<Settings>("settings-changed", (e) => cb(e.payload));

export interface UpdateMetadata {
  currentVersion: string;
  version: string;
}

/** Verlauf eines laufenden Updates. `installing` heißt unter Windows: der
    Installer übernimmt und beendet TippIT gleich — ein `done` kommt dort nie. */
export interface UpdateProgress {
  downloaded: number;
  phase: "done" | "downloading" | "error" | "installing";
  total: number | null;
}

export const onUpdateProgress = (
  cb: (p: UpdateProgress) => void
): Promise<UnlistenFn> =>
  listen<UpdateProgress>("update://progress", (e) => cb(e.payload));

export const checkForUpdate = () =>
  invoke<UpdateMetadata | null>("check_for_update");
export const pendingUpdate = () =>
  invoke<UpdateMetadata | null>("pending_update");
export const installUpdate = () => invoke<void>("install_update");
/** Nach einem macOS-Update die ausgetauschte App neu starten. */
export const restartApp = () => invoke<void>("restart_app");
export const settingsWindowReady = () => invoke<void>("settings_window_ready");
export const updateWindowReady = () => invoke<void>("update_window_ready");
export const closeUpdateWindow = () => invoke<void>("close_update_window");
