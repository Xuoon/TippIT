import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface EntryDto {
  created_at: number;
  has_thumb: boolean;
  kind: number; // 0 Text, 1 Bild, 2 Dateien
  pinned: boolean;
  preview: string;
  size_bytes: number;
  uuid: string;
}

export interface Settings {
  history: {
    max_entries: number;
    capture_images: boolean;
    capture_files: boolean;
    /** Fenstergröße in Prozent der Basisgröße (100 = Standard). */
    window_scale: number;
  };
  /** Abbrechen des Tippens ist fest ESC (kein Setting, s. typing.rs). */
  hotkeys: { paste: string; history: string };
  sounds: boolean;
  sync: {
    deployment_url: string;
    sync_text: boolean;
    sync_settings: boolean;
    sync_images: boolean;
    image_max_bytes: number;
    interval_minutes: number;
    allow_mobile_data: boolean;
    allow_energy_saver: boolean;
    allow_data_saver: boolean;
  };
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

export const entryText = (uuid: string) =>
  invoke<string | null>("entry_text", { uuid });

export const copyEntry = (uuid: string) => invoke<void>("copy_entry", { uuid });
export const typeEntry = (uuid: string) => invoke<void>("type_entry", { uuid });
export const pinEntry = (uuid: string, pinned: boolean) =>
  invoke<void>("pin_entry", { uuid, pinned });
export const deleteEntry = (uuid: string) =>
  invoke<void>("delete_entry", { uuid });
export const clearHistory = () => invoke<void>("clear_history");
export const hideHistoryWindow = () => invoke<void>("hide_history_window");

export const getSettings = () => invoke<Settings>("get_settings");
export const setSettings = (settings: Settings) =>
  invoke<void>("set_settings", { settings });
/** Auslieferungs-Defaults (defaults.json + Rust-Defaults) — einzige Quelle. */
export const getDefaultSettings = () => invoke<Settings>("default_settings");

export interface SyncStatus {
  active: boolean;
  blocked_reason: "data_saver" | "energy_saver" | "mobile_data" | null;
  deployment_url: string;
  group_id: string | null;
}

export const syncStatus = () => invoke<SyncStatus>("sync_status");
export const syncCopyCode = () => invoke<void>("sync_copy_code");
export const syncCreateGroup = () => invoke<SyncStatus>("sync_create_group");
export const syncJoinGroup = (code: string) =>
  invoke<SyncStatus>("sync_join_group", { code });
export const syncLeaveGroup = () => invoke<SyncStatus>("sync_leave_group");

export const onHistoryChanged = (cb: () => void): Promise<UnlistenFn> =>
  listen("history-changed", cb);

export const onSettingsChanged = (
  cb: (s: Settings) => void
): Promise<UnlistenFn> =>
  listen<Settings>("settings-changed", (e) => cb(e.payload));

export interface UpdateMetadata {
  version: string;
}

export const checkForUpdate = () =>
  invoke<UpdateMetadata | null>("check_for_update");
export const pendingUpdate = () =>
  invoke<UpdateMetadata | null>("pending_update");
export const installUpdate = () => invoke<void>("install_update");
export const settingsWindowReady = () => invoke<void>("settings_window_ready");
export const updateWindowReady = () => invoke<void>("update_window_ready");
export const closeUpdateWindow = () => invoke<void>("close_update_window");
