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

/** Monitor, auf dem die Historie öffnet. `name` trägt die Kennung aus
    `MonitorInfo.id`; ein nicht angeschlossener Monitor fällt im Backend auf den
    Mauszeiger zurück. */
export type HistoryScreen =
  | { kind: "cursor" }
  | { kind: "primary" }
  | { kind: "monitor"; name: string };

/** Verschobene Position: Monitor-Kennung plus Lage im freien Raum des
    Arbeitsbereichs (0 = links/oben, 1 = rechts/unten). */
export interface WindowPosition {
  monitor: string;
  x: number;
  y: number;
}

export interface HistorySettings {
  capture_files: boolean;
  /** Formatierung (Clipboard-HTML) sanitisiert mitspeichern. */
  capture_html: boolean;
  capture_images: boolean;
  /** Klick in eine fremde App blendet die Historie aus. */
  close_on_blur: boolean;
  /** Quell-App-IDs, aus denen nichts erfasst wird. */
  excluded_apps: string[];
  max_entries: number;
  /** Einträge nach so vielen Tagen in den Papierkorb legen; 0 = aus. */
  retention_days: number;
  /** Zuletzt per Ziehen gewählte Position; hat Vorrang vor `window_screen`. */
  window_position: WindowPosition | null;
  window_screen: HistoryScreen;
  /** Fensterhöhe in Prozent des Arbeitsbereichs (Breite im Seitenverhältnis). */
  window_size: number;
}

/** Spiegel von `platform::ClickModifier` (serde lowercase). */
export type PaletteModifier = "alt" | "cmd" | "ctrl" | "shift";

/** Palette: Modifier + Klick zeigt die letzten Einträge. */
export interface PaletteSettings {
  count: number;
  enabled: boolean;
  modifier: PaletteModifier;
  /** Klick mit diesem Modifier tippt zeichenweise statt einzufügen. */
  type_modifier: PaletteModifier;
}

export interface Settings {
  history: HistorySettings;
  /** Abbrechen des Tippens ist fest ESC (kein Setting, s. typing.rs). */
  hotkeys: { paste: string; history: string };
  palette: PaletteSettings;
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

/** Sortierung der Liste; Bausteine und Angepinntes stehen immer vorn. */
export type SortKey = "last_copy" | "first_copy" | "copy_count" | "size";
/** Filter über den Eintragstyp hinaus, in Rust ausgewertet (`storage/index.rs`). */
export type RefineId = "pinned" | "snippets" | "links" | "totp";

export interface SearchParams {
  /** Die Seite reicht mindestens bis zu diesem Eintrag. */
  keep?: string;
  kind: number | null;
  limit: number;
  offset: number;
  query: string;
  refine: RefineId | null;
  reverse: boolean;
  sort: SortKey;
}

export interface SearchPage {
  entries: EntryDto[];
  /** Treffer insgesamt, nicht nur auf dieser Seite. */
  total: number;
}

export const searchHistory = (params: SearchParams) =>
  invoke<SearchPage>("search_history", { params });

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

/** UI-Text kopieren und den eigenen Clipboard-Write im Monitor markieren;
    mit `capture` bleibt er unmarkiert und landet in der Historie. */
export const copyText = (text: string, capture = false) =>
  invoke<void>("copy_text", { capture, text });

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
  /** Aktive Einträge über dem Limit; die nächste Kopie entfernt die ältesten ungepinnten. */
  over_limit: number;
  skipped: number;
}

/** Sicherung einlesen und zusammenführen; null = abgebrochen. */
export const importHistory = (password: string) =>
  invoke<ImportReport | null>("import_history", { password });
export const hideHistoryWindow = () => invoke<void>("hide_history_window");

export const getSettings = () => invoke<Settings>("get_settings");
/** `history.window_position` übernimmt das Backend nicht, s. `forgetHistoryPosition`. */
export const setSettings = (settings: Settings) =>
  invoke<void>("set_settings", { settings });
/** Verschobene Position der Historie vergessen. */
export const forgetHistoryPosition = () =>
  invoke<void>("forget_history_position");
/** Auslieferungs-Defaults (defaults.json + Rust-Defaults) — einzige Quelle. */
export const getDefaultSettings = () => invoke<Settings>("default_settings");

export const onHistoryChanged = (cb: () => void): Promise<UnlistenFn> =>
  listen("history-changed", cb);

/** Historie-Fenster wurde eingeblendet. */
export const onHistoryShown = (cb: () => void): Promise<UnlistenFn> =>
  listen("history-shown", cb);

/** Historie-Fenster wurde versteckt. */
export const onHistoryHidden = (cb: () => void): Promise<UnlistenFn> =>
  listen("history-hidden", cb);

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
/** Installation per Tray-Menü angefordert? Liefert true genau einmal. */
export const takeUpdateRequest = () => invoke<boolean>("take_update_request");
/** Tray-Menü fordert die Installation an, während der Hinweis schon offen ist. */
export const onUpdateRequest = (cb: () => void): Promise<UnlistenFn> =>
  listen("update://request", cb);
/** Nach einem macOS-Update die ausgetauschte App neu starten. */
export const restartApp = () => invoke<void>("restart_app");
export const settingsWindowReady = () => invoke<void>("settings_window_ready");

/** Tab, den das Backend beim Öffnen der Einstellungen vorgemerkt hat (einmalig). */
export const takeSettingsTab = () => invoke<string | null>("take_settings_tab");

/** Backend will bei schon offenem Einstellungsfenster einen Tab zeigen. */
export const onSettingsTab = (cb: (tab: string) => void): Promise<UnlistenFn> =>
  listen<string>("settings-tab", (e) => cb(e.payload));

/** Angeschlossener Monitor; Auflösung in physischen Pixeln. */
export interface MonitorInfo {
  height: number;
  /** Kennung für `HistoryScreen`, nicht zum Anzeigen. */
  id: string;
  label: string;
  primary: boolean;
  width: number;
}

export const listMonitors = () => invoke<MonitorInfo[]>("list_monitors");

/** Bedienungshilfen-Freigabe und Installationsort (nur macOS aussagekräftig). */
export interface PermissionStatus {
  bundle_id: string;
  bundle_path: string;
  /** Zwischenablage-Zugriff ab macOS 15.4, sonst null. */
  clipboard_access: "allow" | "ask" | "default" | "deny" | null;
  input_trusted: boolean;
  location: "applications" | "dmg" | "downloads" | "other" | "translocated";
  signature: string;
  supported: boolean;
}

export const permissionStatus = () =>
  invoke<PermissionStatus>("permission_status");

/** Löst den Systemdialog der Bedienungshilfen aus; liefert den aktuellen Status. */
export const requestInputPermission = () =>
  invoke<boolean>("request_input_permission");

export const openPermissionSettings = () =>
  invoke<void>("open_permission_settings");

/** Datenschutz & Sicherheit (Zwischenablage-Zugriff ab macOS 15.4). */
export const openClipboardSettings = () =>
  invoke<void>("open_clipboard_settings");

/** Veralteten Bedienungshilfen-Eintrag entfernen und neu anfragen. */
export const resetInputPermission = () =>
  invoke<boolean>("reset_input_permission");
export const updateWindowReady = () => invoke<void>("update_window_ready");
export const closeUpdateWindow = () => invoke<void>("close_update_window");
