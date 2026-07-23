<script lang="ts">
  import { getVersion } from "@tauri-apps/api/app";
  import { onMount } from "svelte";
  import {
    checkForUpdate,
    clearHistory,
    getDefaultSettings,
    getSettings,
    installUpdate,
    onSettingsChanged,
    pendingUpdate,
    type Settings,
    type SyncStatus,
    setSettings,
    settingsWindowReady,
    syncCopyCode,
    syncCreateGroup,
    syncJoinGroup,
    syncLeaveGroup,
    syncStatus,
    type UpdateMetadata,
  } from "$lib/api";
  import Icon from "$lib/icon.svelte";
  import { formatHotkey, isMacOS, primaryModifierLabel } from "$lib/platform";
  import { initTheme, setThemeMode } from "$lib/theme";
  import changelogRaw from "../../../CHANGELOG.md?raw";
  import "$lib/theme.css";

  let settings = $state<Settings | null>(null);
  let defaults = $state<Settings | null>(null);
  let saveState = $state<"idle" | "saved" | "error">("idle");
  let capturing = $state<"history" | "paste" | null>(null);
  let appVersion = $state("");
  let changelogOpen = $state(false);
  let helpOpen = $state(false);
  let sync = $state<SyncStatus | null>(null);
  let joinCode = $state("");
  let syncBusy = $state(false);
  let syncError = $state("");
  let codeCopied = $state(false);
  let update = $state<UpdateMetadata | null>(null);
  let updateBusy = $state(false);
  let updateMessage = $state("");
  let navQuery = $state("");

  const HK_KW: Record<string, string> = {
    history: "hkHistory",
    paste: "hkPaste",
  };

  // ---- Changelog (CHANGELOG.md wird per Vite ?raw in die App gebündelt) ----
  interface LogGroup {
    items: string[];
    title: string;
  }
  interface LogRelease {
    groups: LogGroup[];
    intro: string[];
    title: string;
  }

  /** Markdown-Inline-Reste entfernen (Links → Text, ** und ` weg). */
  function cleanMd(s: string): string {
    return s
      .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
      .replaceAll("**", "")
      .replaceAll("`", "");
  }

  /** Minimal-Parser für das Keep-a-Changelog-Format (## Release, ### Gruppe, - Punkt). */
  function parseChangelog(md: string): LogRelease[] {
    const releases: LogRelease[] = [];
    let release: LogRelease | null = null;
    let group: LogGroup | null = null;
    for (const raw of md.split("\n")) {
      const line = raw.trimEnd();
      if (line.startsWith("## ")) {
        release = {
          groups: [],
          intro: [],
          title: cleanMd(line.slice(3)).replace("[", "").replace("]", ""),
        };
        releases.push(release);
        group = null;
      } else if (line.startsWith("### ") && release) {
        group = { items: [], title: line.slice(4) };
        release.groups.push(group);
      } else if (line.startsWith("- ")) {
        group?.items.push(cleanMd(line.slice(2)));
      } else if (line !== "" && release && !group && !line.startsWith("#")) {
        release.intro.push(cleanMd(line));
      }
    }
    return releases;
  }

  const CHANGELOG = parseChangelog(changelogRaw);

  const BLOCK_LABELS = {
    data_saver: "Pausiert: Datensparmodus",
    energy_saver: "Pausiert: Energiesparmodus",
    mobile_data: "Pausiert: Mobilfunk",
  } as const;

  const syncState = $derived.by(() => {
    if (!sync?.active) {
      return "off";
    }
    return sync.blocked_reason ? "paused" : "active";
  });

  const syncStateLabel = $derived.by(() => {
    if (!sync?.active) {
      return "Nicht verbunden";
    }
    return sync.blocked_reason ? BLOCK_LABELS[sync.blocked_reason] : "Aktiv";
  });

  const KEYWORDS: Record<string, string> = {
    sounds: "sounds ton akustik signal beep piepsen lautstärke",
    theme: "darstellung theme design aussehen hell dunkel dark light system",
    hkPaste:
      "hotkey tastenkürzel zwischenablage tippen einfügen strg y shortcut",
    hkHistory:
      "hotkey tastenkürzel historie öffnen verlauf strg shift y shortcut",
    hkEsc: "hotkey abbrechen stopp escape esc tippen anhalten",
    preDelay: "startverzögerung verzögerung delay wartezeit vorlauf tippen",
    typeMode:
      "modus zeichenweise auf einmal bulk per char tippen geschwindigkeit",
    charDelay:
      "zeichenabstand tempo geschwindigkeit delay tippen millisekunden",
    trim: "leerraum entfernen trim whitespace leerzeichen kürzen",
    maxEntries: "maximale einträge anzahl limit historie größe aufbewahren",
    winScale:
      "fenstergröße fenster größe skalierung prozent historie breite höhe zoom",
    capImages: "bilder erfassen screenshots aufnehmen historie grafik",
    capFiles: "dateipfade erfassen dateien pfade aufnehmen historie",
    clearHistory: "historie löschen leeren ungepinnt aufräumen entfernen",
    syncConn:
      "sync verbindung code gruppe beitreten verlassen pairing gerät koppeln",
    syncText: "sync text dateipfade umfang synchronisieren inhalte",
    syncSettings: "sync einstellungen synchronisieren übernehmen geräte",
    syncImages: "sync bilder synchronisieren größe kb limit",
    syncInterval: "sync intervall zeitplan sofort minuten stündlich häufigkeit",
    polMobile: "mobilfunk mobil daten richtlinie erlauben netzwerk",
    polEnergy: "energiesparmodus akku batterie richtlinie erlauben",
    polData: "datensparmodus daten sparen richtlinie erlauben",
    syncUrl: "server url convex deployment adresse erweitert endpunkt",
    version: "version update aktualisieren changelog was ist neu prüfen app",
  };

  const q = $derived(navQuery.trim().toLowerCase());
  const hit = (key: string) => q === "" || (KEYWORDS[key] ?? "").includes(q);
  const sectionHit = (keys: string[]) => keys.some(hit);
  const noMatch = $derived(q !== "" && !Object.keys(KEYWORDS).some(hit));

  onMount(() => {
    const stopTheme = initTheme();
    getVersion().then((v) => (appVersion = v));
    Promise.all([
      getSettings(),
      syncStatus(),
      pendingUpdate(),
      getDefaultSettings(),
    ])
      .then(
        async ([loadedSettings, loadedSync, loadedUpdate, loadedDefaults]) => {
          settings = loadedSettings;
          sync = loadedSync;
          update = loadedUpdate;
          defaults = loadedDefaults;
          await settingsWindowReady();
        }
      )
      .catch(async () => {
        await settingsWindowReady();
      });

    const unlisten = onSettingsChanged((incoming) => {
      if (
        JSON.stringify(incoming) !== JSON.stringify($state.snapshot(settings))
      ) {
        settings = incoming;
      }
    });

    // Richtlinien-Pausen (Energiesparmodus etc.) können sich jederzeit ändern;
    // das Badge folgt der Realität, solange das Fenster sichtbar ist.
    const statusTimer = setInterval(() => {
      if (!document.hidden) {
        syncStatus().then((s) => (sync = s));
      }
    }, 30_000);

    return () => {
      clearInterval(statusTimer);
      stopTheme();
      unlisten.then((stop) => stop());
    };
  });

  // Doppelklick auf einen Slider setzt ihn auf den Auslieferungs-Default zurück.
  function resetPreDelay() {
    if (settings && defaults) {
      settings.typing.pre_delay_ms = defaults.typing.pre_delay_ms;
      save();
    }
  }
  function resetCharDelay() {
    if (settings && defaults) {
      settings.typing.char_delay_ms = defaults.typing.char_delay_ms;
      save();
    }
  }
  function resetMaxEntries() {
    if (settings && defaults) {
      settings.history.max_entries = defaults.history.max_entries;
      save();
    }
  }
  function resetWindowScale() {
    if (settings && defaults) {
      settings.history.window_scale = defaults.history.window_scale;
      save();
    }
  }

  const THEMES = [
    { label: "System", value: "system" },
    { label: "Dunkel", value: "dark" },
    { label: "Hell", value: "light" },
  ] as const;

  const TYPE_MODES = [
    { label: "Zeichenweise", value: "per_char" },
    { label: "Auf einmal", value: "bulk" },
  ] as const;

  function setTheme(value: string) {
    if (!settings) {
      return;
    }
    settings.theme = value;
    // Sofort anwenden — nicht erst nach dem Save-Roundtrip.
    setThemeMode(value);
    save();
  }

  function setTypeMode(value: "bulk" | "per_char") {
    if (!settings) {
      return;
    }
    settings.typing.mode = value;
    save();
  }

  type PolicyKey =
    | "allow_mobile_data"
    | "allow_energy_saver"
    | "allow_data_saver";
  // macOS kennt keine Mobilfunk-/Datensparmodus-Erkennung
  // (platform/mac.rs::block_reason) — dort nur den wirksamen Toggle anbieten.
  const POLICIES: { key: PolicyKey; label: string; kw: string }[] = isMacOS
    ? [
        {
          key: "allow_energy_saver",
          label: "Energiesparmodus",
          kw: "polEnergy",
        },
      ]
    : [
        { key: "allow_mobile_data", label: "Mobilfunk", kw: "polMobile" },
        {
          key: "allow_energy_saver",
          label: "Energiesparmodus",
          kw: "polEnergy",
        },
        { key: "allow_data_saver", label: "Datensparmodus", kw: "polData" },
      ];

  function togglePolicy(key: PolicyKey) {
    if (!settings) {
      return;
    }
    settings.sync[key] = !settings.sync[key];
    save();
  }

  type SyncFlagKey = "sync_text" | "sync_settings" | "sync_images";
  function toggleSyncFlag(key: SyncFlagKey) {
    if (!settings) {
      return;
    }
    settings.sync[key] = !settings.sync[key];
    save();
  }

  let saveTimer: ReturnType<typeof setTimeout> | undefined;
  async function save() {
    if (!settings) {
      return;
    }
    try {
      await setSettings($state.snapshot(settings) as Settings);
      saveState = "saved";
    } catch {
      saveState = "error";
    }
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => (saveState = "idle"), 1500);
  }

  async function withSync(action: () => Promise<SyncStatus>) {
    syncBusy = true;
    syncError = "";
    try {
      sync = await action();
    } catch (e) {
      syncError = String(e);
    } finally {
      syncBusy = false;
    }
  }

  async function copyCode() {
    await syncCopyCode();
    codeCopied = true;
    setTimeout(() => (codeCopied = false), 1500);
  }

  async function checkUpdate() {
    updateBusy = true;
    updateMessage = "Prüfe…";
    try {
      update = await checkForUpdate();
      updateMessage = update
        ? `Version ${update.version} ist verfügbar.`
        : "TippIT ist aktuell.";
    } catch (e) {
      updateMessage = `Prüfung fehlgeschlagen: ${String(e)}`;
    } finally {
      updateBusy = false;
    }
  }

  async function startUpdate() {
    updateBusy = true;
    updateMessage = "Update wird geladen und installiert…";
    try {
      await installUpdate();
    } catch (e) {
      updateMessage = `Update fehlgeschlagen: ${String(e)}`;
      updateBusy = false;
    }
  }

  const F_KEY_PATTERN = /^F\d{1,2}$/i;
  const LETTER_DIGIT_PATTERN = /^[a-z0-9]$/;
  // Layoutbewusst über event.key: event.code liefert die PHYSISCHE Taste im
  // US-Layout — auf QWERTZ wären Y und Z vertauscht. Registriert wird ebenfalls
  // layoutbewusst (Windows: virtuelle Keys; macOS: platform::resolve_hotkey).
  function keyFromEvent(event: KeyboardEvent): string | null {
    const key = event.key.toLowerCase();
    if (LETTER_DIGIT_PATTERN.test(key)) {
      return key;
    }
    if (F_KEY_PATTERN.test(event.key)) {
      return key;
    }
    // Shift+Ziffer liefert als key ein Sonderzeichen ("!", "§", …) —
    // dann hilft der physische Code weiter.
    if (event.code.startsWith("Digit")) {
      return event.code.slice(5);
    }
    return null;
  }

  function captureHotkey(event: KeyboardEvent) {
    if (event.key === "Escape" && (changelogOpen || helpOpen)) {
      changelogOpen = false;
      helpOpen = false;
      return;
    }
    if (!(capturing && settings)) {
      return;
    }
    event.preventDefault();
    if (event.key === "Escape") {
      capturing = null;
      return;
    }
    const key = keyFromEvent(event);
    if (!key) {
      return;
    }
    const parts: string[] = [];
    if (event.ctrlKey) {
      parts.push("ctrl");
    }
    if (event.shiftKey) {
      parts.push("shift");
    }
    if (event.altKey) {
      parts.push("alt");
    }
    if (event.metaKey) {
      parts.push("super");
    }
    if (parts.length === 0) {
      return;
    }
    parts.push(key);
    settings.hotkeys[capturing] = parts.join("+");
    capturing = null;
    save();
  }

  async function onClearHistory() {
    // biome-ignore lint/suspicious/noAlert: bewusster nativer Bestätigungsdialog
    if (confirm("Alle ungepinnten Einträge löschen?")) {
      await clearHistory();
    }
  }
</script>

<svelte:window onkeydown={captureHotkey} />

{#if settings}
  <main>
    <!-- Kopfzeile: Suche, Speicher-Quittung, Hilfe -->
    <header class="top">
      <div class="search">
        <Icon name="search" size={16} />
        <input
          placeholder="Einstellung suchen…"
          spellcheck="false"
          bind:value={navQuery}
        >
      </div>
      <span
        class="savebadge"
        class:error={saveState === "error"}
        class:on={saveState !== "idle"}
        class:saved={saveState === "saved"}
      >
        {#if saveState === "saved"}
          <Icon name="check" size={12} />Gespeichert
        {:else if saveState === "error"}
          <Icon name="alert" size={12} />Fehler
        {/if}
      </span>
      <button
        aria-label="Hilfe"
        class="iconbtn"
        onclick={() => (helpOpen = true)}
        title="Hilfe"
        type="button"
      >
        <Icon name="help" size={16} />
      </button>
    </header>

    <div class="scroll" class:searching={q !== ""}>
      <!-- Allgemein -->
      {#if sectionHit(["sounds", "theme"])}
        <section>
          <div class="glabel">Allgemein</div>
          {#if hit("sounds")}
            <label class="row">
              <span class="row-label">Sounds</span>
              <span class="switch">
                <input
                  onchange={save}
                  type="checkbox"
                  bind:checked={settings.sounds}
                >
                <span class="track"></span>
                <span class="knob"></span>
              </span>
            </label>
          {/if}
          {#if hit("theme")}
            <div class="row">
              <span class="row-label">Darstellung</span>
              <span class="chipgroup">
                {#each THEMES as t (t.value)}
                  <button
                    class="chip"
                    onclick={() => setTheme(t.value)}
                    type="button"
                    class:on={settings.theme === t.value}
                  >
                    {t.label}
                  </button>
                {/each}
              </span>
            </div>
          {/if}
        </section>
      {/if}

      <!-- Hotkeys -->
      {#if sectionHit(["hkPaste", "hkHistory", "hkEsc"])}
        <section>
          <div class="glabel">Hotkeys</div>
          {#each [["paste", "Zwischenablage tippen"], ["history", "Historie öffnen"]] as [ key, label ] (key)}
            {#if hit(HK_KW[key])}
              <div class="row">
                <span class="row-label">{label}</span>
                <button
                  class="btn hotkey"
                  onclick={() =>
                    (capturing =
                      capturing === key ? null : (key as "history" | "paste"))}
                  type="button"
                  class:recording={capturing === key}
                >
                  {capturing === key
                    ? "Tasten drücken…"
                    : formatHotkey(settings.hotkeys[key as "history" | "paste"])}
                </button>
              </div>
            {/if}
          {/each}
          {#if hit("hkEsc")}
            <div class="row">
              <span class="row-label">Tippen abbrechen</span>
              <!-- Fest ESC (kein Setting): wird nur während eines
                   Tipp-Vorgangs global registriert. -->
              <kbd class="fixed-key">Esc</kbd>
            </div>
          {/if}
        </section>
      {/if}

      <!-- Tippen -->
      {#if sectionHit(["preDelay", "typeMode", "charDelay", "trim"])}
        <section>
          <div class="glabel">Tippen</div>
          {#if hit("preDelay")}
            <label class="row slider">
              <span class="row-label">Startverzögerung</span>
              <input
                max="3000"
                min="0"
                onchange={save}
                ondblclick={resetPreDelay}
                step="100"
                title="Doppelklick: Standard"
                type="range"
                bind:value={settings.typing.pre_delay_ms}
              >
              <output>{settings.typing.pre_delay_ms} ms</output>
            </label>
          {/if}
          {#if hit("typeMode")}
            <div class="row">
              <span class="row-label">Modus</span>
              <span class="chipgroup">
                {#each TYPE_MODES as m (m.value)}
                  <button
                    class="chip"
                    onclick={() => setTypeMode(m.value)}
                    type="button"
                    class:on={settings.typing.mode === m.value}
                  >
                    {m.label}
                  </button>
                {/each}
              </span>
            </div>
          {/if}
          {#if hit("charDelay") && settings.typing.mode === "per_char"}
            <label class="row slider">
              <span class="row-label">Zeichenabstand</span>
              <input
                max="100"
                min="1"
                onchange={save}
                ondblclick={resetCharDelay}
                title="Doppelklick: Standard"
                type="range"
                bind:value={settings.typing.char_delay_ms}
              >
              <output>{settings.typing.char_delay_ms} ms</output>
            </label>
          {/if}
          {#if hit("trim")}
            <label class="row">
              <span class="row-label">Leerraum entfernen</span>
              <span class="switch">
                <input
                  onchange={save}
                  type="checkbox"
                  bind:checked={settings.typing.trim}
                >
                <span class="track"></span>
                <span class="knob"></span>
              </span>
            </label>
          {/if}
        </section>
      {/if}

      <!-- Historie -->
      {#if sectionHit(["maxEntries", "winScale", "capImages", "capFiles", "clearHistory"])}
        <section>
          <div class="glabel">Historie</div>
          {#if hit("maxEntries")}
            <label class="row slider">
              <span class="row-label">Maximale Einträge</span>
              <input
                max="5000"
                min="100"
                onchange={save}
                ondblclick={resetMaxEntries}
                step="100"
                title="Doppelklick: Standard"
                type="range"
                bind:value={settings.history.max_entries}
              >
              <output>{settings.history.max_entries}</output>
            </label>
          {/if}
          {#if hit("winScale")}
            <label class="row slider">
              <span class="row-label">Fenstergröße</span>
              <input
                max="150"
                min="70"
                onchange={save}
                ondblclick={resetWindowScale}
                step="5"
                title="Doppelklick: Standard"
                type="range"
                bind:value={settings.history.window_scale}
              >
              <output>{settings.history.window_scale} %</output>
            </label>
          {/if}
          {#if hit("capImages")}
            <label class="row">
              <span class="row-label">Bilder erfassen</span>
              <span class="switch">
                <input
                  onchange={save}
                  type="checkbox"
                  bind:checked={settings.history.capture_images}
                >
                <span class="track"></span>
                <span class="knob"></span>
              </span>
            </label>
          {/if}
          {#if hit("capFiles")}
            <label class="row">
              <span class="row-label">Dateipfade erfassen</span>
              <span class="switch">
                <input
                  onchange={save}
                  type="checkbox"
                  bind:checked={settings.history.capture_files}
                >
                <span class="track"></span>
                <span class="knob"></span>
              </span>
            </label>
          {/if}
          {#if hit("clearHistory")}
            <div class="row">
              <span class="row-label">Ungepinnte Einträge</span>
              <button class="btn danger" onclick={onClearHistory} type="button">
                <Icon name="trash" size={14} />Löschen
              </button>
            </div>
          {/if}
        </section>
      {/if}

      <!-- Synchronisierung -->
      {#if sectionHit(["syncConn", "syncText", "syncSettings", "syncImages", "syncInterval", ...POLICIES.map((p) => p.kw), "syncUrl"])}
        <section>
          <div class="glabel">
            Synchronisierung
            <span
              class="badge syncbadge"
              class:active={syncState === "active"}
              class:off={syncState === "off"}
              class:paused={syncState === "paused"}
            >
              <span class="dot"></span>{syncStateLabel}
            </span>
          </div>
          {#if hit("syncConn")}
            {#if sync?.active}
              <div class="row actions">
                <span class="row-label">Verbindung</span>
                <span class="grow"></span>
                <button
                  class="btn"
                  disabled={syncBusy}
                  onclick={copyCode}
                  type="button"
                >
                  <Icon name="copy" size={14} />
                  {codeCopied ? "Kopiert" : "Code kopieren"}
                </button>
                <button
                  class="btn danger"
                  disabled={syncBusy}
                  onclick={() => withSync(syncLeaveGroup)}
                  type="button"
                >
                  Gruppe verlassen
                </button>
              </div>
            {:else}
              <div class="row actions">
                <input
                  class="input join"
                  placeholder="TIPPIT-Code"
                  spellcheck="false"
                  type="text"
                  bind:value={joinCode}
                >
                <button
                  class="btn primary"
                  disabled={syncBusy ||
                    joinCode.length < 20 ||
                    !settings.sync.deployment_url}
                  onclick={() => withSync(() => syncJoinGroup(joinCode))}
                  type="button"
                >
                  Beitreten
                </button>
                <button
                  class="btn"
                  disabled={syncBusy || !settings.sync.deployment_url}
                  onclick={() => withSync(syncCreateGroup)}
                  type="button"
                >
                  Neue Gruppe
                </button>
              </div>
            {/if}
            {#if syncError}
              <div class="row error-row">
                <Icon name="alert" size={14} />
                <span>{syncError}</span>
              </div>
            {/if}
          {/if}

          {#if sectionHit(["syncText", "syncSettings", "syncImages"])}
            <div class="row actions">
              <span class="row-label">Umfang</span>
              <span class="grow"></span>
              {#if hit("syncText")}
                <button
                  class="chip"
                  onclick={() => toggleSyncFlag("sync_text")}
                  type="button"
                  class:on={settings.sync.sync_text}
                >
                  <Icon
                    name={settings.sync.sync_text ? "check" : "x"}
                    size={12}
                  />Text &amp; Dateipfade
                </button>
              {/if}
              {#if hit("syncSettings")}
                <button
                  class="chip"
                  onclick={() => toggleSyncFlag("sync_settings")}
                  type="button"
                  class:on={settings.sync.sync_settings}
                >
                  <Icon
                    name={settings.sync.sync_settings ? "check" : "x"}
                    size={12}
                  />Einstellungen
                </button>
              {/if}
              {#if hit("syncImages")}
                <button
                  class="chip"
                  onclick={() => toggleSyncFlag("sync_images")}
                  type="button"
                  class:on={settings.sync.sync_images}
                >
                  <Icon
                    name={settings.sync.sync_images ? "check" : "x"}
                    size={12}
                  />Bilder bis
                  {Math.round(settings.sync.image_max_bytes / 1024)}
                  KB
                </button>
              {/if}
            </div>
          {/if}
          {#if hit("syncInterval")}
            <label class="row">
              <span class="row-label">Synchronisieren</span>
              <span class="select">
                <select
                  onchange={save}
                  bind:value={settings.sync.interval_minutes}
                >
                  <option value={0}>Sofort</option>
                  <option value={1}>Jede Minute</option>
                  <option value={5}>Alle 5 Minuten</option>
                  <option value={15}>Alle 15 Minuten</option>
                  <option value={30}>Alle 30 Minuten</option>
                  <option value={60}>Stündlich</option>
                </select>
                <Icon name="chevron-down" size={12} />
              </span>
            </label>
          {/if}
          {#if sectionHit(POLICIES.map((p) => p.kw))}
            <div
              class="row actions"
              title="Sync läuft in diesen Modi nur, wenn erlaubt."
            >
              <span class="row-label">Erlaubt bei</span>
              <span class="grow"></span>
              {#each POLICIES as policy (policy.key)}
                {#if hit(policy.kw)}
                  <button
                    class="chip"
                    onclick={() => togglePolicy(policy.key)}
                    type="button"
                    class:on={settings.sync[policy.key]}
                  >
                    <Icon
                      name={settings.sync[policy.key] ? "check" : "x"}
                      size={12}
                    />{policy.label}
                  </button>
                {/if}
              {/each}
            </div>
          {/if}
          {#if hit("syncUrl")}
            <details>
              <summary class="row">
                <span class="row-label">Erweitert</span>
                <Icon name="chevron-down" size={12} />
              </summary>
              <label class="row url-row">
                <span class="row-label">Server-URL</span>
                <input
                  class="input mono url"
                  onchange={save}
                  placeholder="https://….convex.cloud"
                  type="text"
                  bind:value={settings.sync.deployment_url}
                >
              </label>
            </details>
          {/if}
        </section>
      {/if}

      <!-- App -->
      {#if hit("version")}
        <section>
          <div class="glabel">App</div>
          <div class="row">
            <span class="row-label">
              TippIT {appVersion ? `v${appVersion}` : ""}
            </span>
            <span class="grow"></span>
            <button
              class="whatsnew"
              onclick={() => (changelogOpen = true)}
              type="button"
            >
              Was ist neu?
            </button>
            <button
              class="btn"
              disabled={updateBusy}
              onclick={update ? startUpdate : checkUpdate}
              title={updateMessage || undefined}
              type="button"
              class:primary={Boolean(update)}
            >
              <Icon name="download" size={13} />
              {update ? `Update auf ${update.version}` : "Nach Updates suchen"}
            </button>
          </div>
          {#if updateMessage}
            <div class="row meta-row">{updateMessage}</div>
          {/if}
        </section>
      {/if}

      {#if noMatch}
        <p class="noresults">Keine Treffer für „{navQuery}"</p>
      {/if}

      <div class="credit">TippIT · Sven Labitzki</div>
    </div>
  </main>

  <!-- Hilfe-Overlay: bewusst kein fester Bereich mehr — nur bei Bedarf. -->
  {#if helpOpen}
    <div class="modal-backdrop">
      <div class="modal">
        <div class="modal-head">
          <h2>Hilfe</h2>
          <button
            aria-label="Schließen"
            class="modal-close"
            onclick={() => (helpOpen = false)}
            title="Schließen (Esc)"
            type="button"
          >
            <Icon name="x" size={14} />
          </button>
        </div>
        <div class="modal-body">
          <div class="hlp-group">So funktioniert TippIT</div>
          <p class="hlp-text">
            Zielfeld fokussieren, Hotkey drücken — nach der Startverzögerung
            wird die Zwischenablage als echte Tastatureingaben getippt
            (funktioniert auch in RDP, VMs und Feldern, die Einfügen
            blockieren).
          </p>
          {#each [[formatHotkey(settings.hotkeys.paste), "Zwischenablage tippen"], [formatHotkey(settings.hotkeys.history), "Historie öffnen"], ["Esc", "Laufendes Tippen abbrechen"]] as [ keys, what ] (what)}
            <div class="hlp-row">
              <span>{what}</span>
              <kbd class="fixed-key">{keys}</kbd>
            </div>
          {/each}

          <div class="hlp-group">Tastatur in der Historie</div>
          {#each [["↑ ↓", "Eintrag wählen"], ["Enter", "Kopieren und schließen"], [`${primaryModifierLabel}+Enter`, "Als Tastatur tippen"], ["⇧+Enter", "Aktion: Link/Datei öffnen, Text extrahieren"], [`${primaryModifierLabel}+P`, "Anpinnen / Pin lösen"], [`${primaryModifierLabel}+Entf`, "Eintrag löschen"], ["Tab", "Filter wechseln"], ["Esc", "Fenster schließen"]] as [ keys, what ] (keys)}
            <div class="hlp-row">
              <span>{what}</span>
              <kbd class="fixed-key">{keys}</kbd>
            </div>
          {/each}

          <div class="hlp-group">Wenn nichts getippt wird</div>
          {#if isMacOS}
            <p class="hlp-text">
              TippIT braucht die Bedienungshilfen-Berechtigung:
              Systemeinstellungen → Datenschutz &amp; Sicherheit →
              Bedienungshilfen → TippIT erlauben. Ohne sie verwirft macOS die
              Eingaben still (Fehlerton beim Versuch).
            </p>
          {:else}
            <p class="hlp-text">
              In Fenster, die als Administrator laufen, kann TippIT ohne eigene
              Adminrechte nicht tippen (Windows-Schutz). Auch prüfen: Zielfeld
              wirklich fokussiert?
            </p>
          {/if}
          <p class="hlp-text">
            Daten &amp; Logs liegen unter <code>~/.labi/tippit/</code> — die
            Log-Dateien helfen bei der Fehlersuche.
          </p>
        </div>
      </div>
    </div>
  {/if}

  {#if changelogOpen}
    <div class="modal-backdrop">
      <div class="modal">
        <div class="modal-head">
          <h2>Was ist neu?</h2>
          <button
            aria-label="Schließen"
            class="modal-close"
            onclick={() => (changelogOpen = false)}
            title="Schließen (Esc)"
            type="button"
          >
            <Icon name="x" size={14} />
          </button>
        </div>
        <div class="modal-body">
          {#each CHANGELOG as release (release.title)}
            <div class="log-release">
              <h3>
                {release.title.startsWith("Unreleased")
                  ? "Unveröffentlicht"
                  : release.title}
              </h3>
              {#each release.intro as line (line)}
                <p class="log-intro">{line}</p>
              {/each}
              {#each release.groups as group (group.title)}
                <div class="log-group">{group.title}</div>
                <ul>
                  {#each group.items as item (item)}
                    <li>{item}</li>
                  {/each}
                </ul>
              {/each}
            </div>
          {/each}
        </div>
      </div>
    </div>
  {/if}
{/if}

<style>
  :global(body) {
    margin: 0;
    overflow: hidden;
    font-family: var(--font-ui);
    user-select: none;
  }

  main {
    display: flex;
    flex-direction: column;
    height: 100vh;
    overflow: hidden;
    background: var(--bg-base);
  }

  /* ---- Kopfzeile ---- */
  .top {
    display: flex;
    flex: none;
    gap: 10px;
    align-items: center;
    height: var(--search-h);
    padding: 0 10px 0 16px;
    border-bottom: 1px solid var(--border);
  }
  .search {
    display: flex;
    flex: 1;
    gap: 8px;
    align-items: center;
    min-width: 0;
    height: 100%;
    color: var(--fg-dim);
  }
  .search input {
    flex: 1;
    min-width: 0;
    font-size: var(--fs-control);
    color: var(--fg);
    outline: none;
    background: transparent;
    border: 0;
  }
  .search input::placeholder {
    color: var(--fg-placeholder);
  }
  .iconbtn {
    display: grid;
    flex: none;
    place-items: center;
    width: 28px;
    height: 28px;
    color: var(--fg-dim);
    cursor: pointer;
    background: transparent;
    border: 0;
    border-radius: var(--r-md);
  }
  .iconbtn:hover {
    color: var(--fg);
    background: var(--row-hover);
  }
  .savebadge {
    display: inline-flex;
    flex: none;
    gap: 6px;
    align-items: center;
    height: 22px;
    padding: 0 10px;
    font: 500 var(--fs-micro) / 1 var(--font-ui);
    border-radius: var(--r-md);
    opacity: 0;
    transition: opacity var(--t-base) linear;
  }
  .savebadge.on {
    opacity: 1;
  }
  .savebadge.saved {
    color: var(--success);
    background: var(--success-soft);
  }
  .savebadge.error {
    color: var(--danger);
    background: var(--danger-soft);
  }

  /* ---- Flache Liste ---- */
  .scroll {
    flex: 1;
    min-height: 0;
    padding-bottom: 8px;
    overflow-y: auto;
  }
  section {
    border-bottom: 1px solid var(--border);
  }
  section:last-of-type {
    border-bottom: 0;
  }
  .glabel {
    display: flex;
    gap: 10px;
    align-items: center;
    justify-content: space-between;
    padding: 16px 16px 6px;
    font: 600 var(--fs-micro) / 1 var(--font-ui);
    color: var(--fg-dim);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .row {
    display: flex;
    gap: 12px;
    align-items: center;
    justify-content: space-between;
    min-height: 40px;
    padding: 2px 16px;
  }
  .row + .row,
  .row + details {
    border-top: 1px solid var(--border-soft);
  }
  .row-label {
    flex: none;
    font: 450 var(--fs-label) / 1.35 var(--font-ui);
    color: var(--fg);
  }
  /* Suchmodus: sichtbare Zeilen SIND die Treffer — Labels gelb markieren. */
  .searching .row-label {
    padding: 2px 5px;
    margin: -2px -5px;
    background: var(--mark);
    border-radius: var(--r-sm);
  }
  .grow {
    flex: 1;
  }
  .row.actions {
    flex-wrap: wrap;
    justify-content: flex-start;
    padding-top: 6px;
    padding-bottom: 6px;
  }
  /* Doppelklick setzt auf den Auslieferungs-Default zurück (resetXyz-Handler). */
  .row.slider input[type="range"] {
    flex: 1;
    min-width: 0;
    height: 4px;
    margin: 0;
    accent-color: var(--accent);
  }
  .row.slider output {
    flex: none;
    min-width: 58px;
    font: 400 var(--fs-button) / 1 var(--font-ui);
    font-variant-numeric: tabular-nums;
    color: var(--accent-text);
    text-align: right;
  }
  .meta-row {
    min-height: 26px;
    font-size: var(--fs-meta);
    color: var(--fg-dim);
  }
  .error-row {
    color: var(--danger);
  }
  .error-row span {
    font-size: var(--fs-control);
    color: var(--danger);
  }

  /* ---- Buttons ---- */
  .btn {
    display: inline-flex;
    gap: 6px;
    align-items: center;
    justify-content: center;
    height: 28px;
    padding: 0 12px;
    font: 500 var(--fs-button) / 1 var(--font-ui);
    color: var(--fg-body);
    cursor: pointer;
    background: var(--bg-raised);
    border: 0;
    border-radius: var(--r-md);
    transition:
      background var(--t-fast) linear,
      color var(--t-fast) linear;
  }
  .btn:hover {
    color: var(--fg);
    background: var(--bg-hover);
  }
  .btn.primary {
    color: var(--fg-on-accent);
    background: var(--accent);
  }
  .btn.primary:hover {
    background: var(--accent-hover);
  }
  .btn.danger {
    color: var(--danger);
    background: transparent;
  }
  .btn.danger:hover {
    color: var(--danger-hover);
    background: var(--danger-soft);
  }
  .btn:disabled {
    pointer-events: none;
    cursor: default;
    opacity: 0.45;
  }
  .btn:focus-visible {
    outline: none;
    box-shadow: var(--shadow-focus);
  }
  .btn.hotkey {
    min-width: 156px;
    font-variant-numeric: tabular-nums;
    letter-spacing: 0.3px;
  }
  .btn.hotkey.recording {
    color: var(--accent-text);
    background: var(--accent-soft);
  }
  .whatsnew {
    padding: 0;
    font-size: var(--fs-meta);
    color: var(--accent-text);
    cursor: pointer;
    background: transparent;
    border: 0;
  }
  .whatsnew:hover {
    text-decoration: underline;
  }

  /* ---- Switch ---- */
  .switch {
    position: relative;
    flex: none;
    width: 34px;
    height: 20px;
  }
  .switch input {
    position: absolute;
    inset: 0;
    margin: 0;
    cursor: pointer;
    opacity: 0;
  }
  .track {
    display: block;
    width: 34px;
    height: 20px;
    background: var(--bg-strong);
    border-radius: var(--r-full);
    transition: background var(--t-base) linear;
  }
  .knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 16px;
    height: 16px;
    background: var(--fg-on-accent);
    border-radius: 50%;
    transition: transform var(--t-base) ease-out;
  }
  .switch input:checked ~ .track {
    background: var(--accent);
  }
  .switch input:checked ~ .knob {
    transform: translateX(14px);
  }
  .switch input:focus-visible ~ .track {
    box-shadow: var(--shadow-focus);
  }

  /* ---- Select ---- */
  .select {
    position: relative;
    display: inline-flex;
    align-items: center;
    color: var(--fg-dim);
  }
  .select select {
    min-width: 156px;
    height: 30px;
    padding: 0 30px 0 10px;
    font: 400 var(--fs-control) / 1 var(--font-ui);
    color: var(--fg-body);
    appearance: none;
    cursor: pointer;
    outline: none;
    background: var(--bg-raised);
    border: 0;
    border-radius: var(--r-md);
  }
  .select :global(.ic) {
    position: absolute;
    right: 9px;
    color: var(--fg-dim);
    pointer-events: none;
  }
  .select select:focus-visible {
    box-shadow: inset 0 0 0 1px var(--border-focus);
  }

  /* ---- Textfeld ---- */
  .input {
    height: 30px;
    padding: 0 10px;
    font: 400 var(--fs-control) / 1 var(--font-ui);
    color: var(--fg);
    outline: none;
    background: var(--bg-raised);
    border: 0;
    border-radius: var(--r-md);
  }
  .input::placeholder {
    color: var(--fg-placeholder);
  }
  .input:focus-visible {
    box-shadow: inset 0 0 0 1px var(--border-focus);
  }
  .input.mono {
    font-family: var(--font-mono);
  }
  .input.join {
    flex: 1;
    min-width: 150px;
  }
  .input.url {
    flex: 1;
    min-width: 0;
  }
  .url-row {
    padding-bottom: 10px;
  }

  /* ---- Chips ---- */
  .chipgroup {
    display: inline-flex;
    gap: 6px;
  }
  .chip {
    display: inline-flex;
    gap: 6px;
    align-items: center;
    height: 26px;
    padding: 0 10px;
    font: 500 var(--fs-button) / 1 var(--font-ui);
    color: var(--fg-muted);
    cursor: pointer;
    background: var(--bg-raised);
    border: 0;
    border-radius: var(--r-full);
    transition:
      background var(--t-fast) linear,
      color var(--t-fast) linear;
  }
  .chip:hover {
    color: var(--fg);
    background: var(--bg-hover);
  }
  .chip.on {
    color: var(--accent-text);
    background: var(--accent-soft);
  }
  .chip:focus-visible {
    outline: none;
    box-shadow: var(--shadow-focus);
  }

  /* ---- Badge (Sync-Status im Gruppenlabel) ---- */
  .badge {
    display: inline-flex;
    gap: 6px;
    align-items: center;
    height: 20px;
    padding: 0 8px;
    font: 500 var(--fs-micro) / 1 var(--font-ui);
    text-transform: none;
    letter-spacing: normal;
    border-radius: var(--r-md);
  }
  .badge .dot {
    flex: none;
    width: 6px;
    height: 6px;
    background: currentColor;
    border-radius: 50%;
  }
  .syncbadge.off {
    color: var(--fg-muted);
    background: var(--bg-raised);
  }
  .syncbadge.active {
    color: var(--success);
    background: var(--success-soft);
  }
  .syncbadge.paused {
    color: var(--warn);
    background: var(--warn-soft);
  }

  /* ---- Details / Erweitert ---- */
  details > summary {
    cursor: pointer;
    list-style: none;
  }
  details > summary::-webkit-details-marker {
    display: none;
  }
  details > summary.row :global(.ic) {
    color: var(--fg-dim);
    transform: rotate(-90deg);
    transition: transform var(--t-fast) ease-out;
  }
  details[open] > summary.row :global(.ic) {
    transform: rotate(0deg);
  }
  details > summary .row-label {
    color: var(--fg-muted);
  }

  .noresults {
    padding-top: 38%;
    margin: 0;
    font-size: var(--fs-control);
    color: var(--fg-dim);
    text-align: center;
  }
  .credit {
    padding: 14px 16px 6px;
    font-size: var(--fs-code);
    color: var(--fg-disabled);
  }

  /* ---- Feste Tastenanzeige ---- */
  .fixed-key {
    padding: 3px 8px;
    font: 500 var(--fs-micro) / 1 var(--font-ui);
    color: var(--fg-muted);
    white-space: nowrap;
    background: var(--bg-strong);
    border-radius: var(--r-sm);
  }

  /* ---- Overlays (Hilfe, Changelog) ---- */
  .modal-backdrop {
    position: fixed;
    inset: 0;
    z-index: 10;
    display: grid;
    place-items: center;
    background: var(--overlay);
  }
  .modal {
    display: flex;
    flex-direction: column;
    width: min(520px, calc(100vw - 48px));
    max-height: calc(100vh - 72px);
    overflow: hidden;
    background: var(--bg-base);
    border: 1px solid var(--border);
    border-radius: var(--r-xl);
    box-shadow: var(--shadow-overlay);
  }
  .modal-head {
    display: flex;
    flex: none;
    align-items: center;
    justify-content: space-between;
    padding: 14px 18px;
    border-bottom: 1px solid var(--border);
  }
  .modal-head h2 {
    margin: 0;
    font: 600 var(--fs-title) / 1 var(--font-ui);
    color: var(--fg);
  }
  .modal-close {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    color: var(--fg-dim);
    cursor: pointer;
    background: transparent;
    border: 0;
    border-radius: var(--r-md);
  }
  .modal-close:hover {
    color: var(--fg);
    background: var(--row-hover);
  }
  .modal-body {
    flex: 1;
    min-height: 0;
    padding: 6px 18px 18px;
    overflow-y: auto;
    user-select: text;
  }

  /* ---- Hilfe-Inhalt ---- */
  .hlp-group {
    margin: 18px 0 6px;
    font: 600 var(--fs-micro) / 1 var(--font-ui);
    color: var(--fg-dim);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  .hlp-text {
    margin: 4px 0;
    font-size: var(--fs-control);
    line-height: 1.5;
    color: var(--fg-body);
  }
  .hlp-text code {
    font-family: var(--font-mono);
    font-size: var(--fs-meta);
    color: var(--fg-muted);
  }
  .hlp-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    min-height: 28px;
    font-size: var(--fs-control);
    color: var(--fg-body);
  }
  .hlp-row + .hlp-row {
    border-top: 1px solid var(--border-soft);
  }

  /* ---- Changelog-Inhalt ---- */
  .log-release h3 {
    margin: 18px 0 4px;
    font: 600 var(--fs-label) / 1 var(--font-ui);
    color: var(--fg);
  }
  .log-intro {
    margin: 4px 0;
    font-size: var(--fs-control);
    color: var(--fg-body);
  }
  .log-group {
    margin: 10px 0 2px;
    font: 600 var(--fs-micro) / 1 var(--font-ui);
    color: var(--fg-dim);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .log-release ul {
    padding-left: 18px;
    margin: 4px 0;
  }
  .log-release li {
    margin: 3px 0;
    font-size: var(--fs-control);
    line-height: 1.5;
    color: var(--fg-body);
  }
</style>
