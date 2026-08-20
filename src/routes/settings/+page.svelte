<script lang="ts">
  import { getVersion } from "@tauri-apps/api/app";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount } from "svelte";
  import {
    checkForUpdate,
    clearHistory,
    exportHistory,
    getDefaultSettings,
    getSettings,
    type ImportReport,
    importHistory,
    installUpdate,
    onSettingsChanged,
    onUpdateProgress,
    openDataDir,
    pendingUpdate,
    type Settings,
    setSettings,
    settingsWindowReady,
    type UpdateMetadata,
    type UpdateProgress,
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

  // ---- Sicherung (Export/Import) ----
  let backupPassword = $state("");
  let backupBusy = $state(false);
  let backupMessage = $state("");
  let backupError = $state("");

  // ---- Ausschlussliste ----
  let excludeInput = $state("");
  let update = $state<UpdateMetadata | null>(null);
  let updateBusy = $state(false);
  let updateMessage = $state("");
  let updateProgress = $state<UpdateProgress | null>(null);
  /** Fortschritt in Prozent; null = unbestimmt (Server ohne Content-Length). */
  const updatePercent = $derived.by(() => {
    const total = updateProgress?.total ?? 0;
    if (!(updateProgress && total > 0)) {
      return null;
    }
    return Math.min(100, Math.round((updateProgress.downloaded / total) * 100));
  });
  let navQuery = $state("");
  let activeTab = $state<"allgemein" | "tippen" | "historie" | "daten">(
    "allgemein"
  );

  /** Hero-Hotkeys: Klick auf die Keycaps startet direkt die Aufnahme. */
  const HERO_HOTKEYS = [
    { key: "paste", kw: "hkPaste", label: "Zwischenablage tippen" },
    { key: "history", kw: "hkHistory", label: "Historie öffnen" },
  ] as const;

  const TABS = [
    { icon: "sliders", id: "allgemein", label: "Allgemein" },
    { icon: "cursor-text", id: "tippen", label: "Tippen" },
    { icon: "clock", id: "historie", label: "Historie" },
    { icon: "download", id: "daten", label: "Daten" },
  ] as const;

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

  /** Farbschlüssel je Changelog-Gruppe — Neues grün, Geändertes blau,
      Entferntes rot, Behobenes gelb. Unbekannte Überschriften bleiben neutral. */
  const LOG_TONES: Record<string, string> = {
    Hinzugefügt: "add",
    Geändert: "change",
    Entfernt: "remove",
    Behoben: "fix",
  };
  const logTone = (title: string) => LOG_TONES[title.trim()] ?? "";

  /** Release-Titel „2.0.0 – 2026-08-19" in Version und Datum trennen. */
  function releaseName(title: string): string {
    const name = title.split(" – ")[0].trim();
    return name.startsWith("Unreleased") ? "Unveröffentlicht" : name;
  }
  function releaseDate(title: string): string {
    return title.split(" – ")[1]?.trim() ?? "";
  }

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
    capHtml:
      "formatierung rich text html farben fett kursiv erfassen mitspeichern",
    retention:
      "aufbewahrung frist alter tage automatisch aufräumen löschen papierkorb",
    excludeApps:
      "ausschluss ausnahme programme apps passwortmanager banking ignorieren nicht erfassen",
    clearHistory: "historie löschen leeren ungepinnt aufräumen entfernen",
    backup:
      "sicherung export import backup umzug übertragen datei passwort verschlüsselt gerät wechseln",
    dataDir: "datenverzeichnis ordner speicherort dateien öffnen logs",
    version: "version update aktualisieren changelog was ist neu prüfen app",
  };

  const q = $derived(navQuery.trim().toLowerCase());
  const hit = (key: string) => q === "" || (KEYWORDS[key] ?? "").includes(q);
  const noMatch = $derived(q !== "" && !Object.keys(KEYWORDS).some(hit));

  /** Zeile sichtbar? Ohne Suche entscheidet der Tab, mit Suche der Treffer. */
  const show = (key: string, tab: string) =>
    q === "" ? activeTab === tab : (KEYWORDS[key] ?? "").includes(q);
  const showSection = (keys: string[], tab: string) =>
    q === "" ? activeTab === tab : keys.some((k) => KEYWORDS[k]?.includes(q));

  onMount(() => {
    const stopTheme = initTheme();
    getVersion().then((v) => (appVersion = v));
    Promise.all([getSettings(), pendingUpdate(), getDefaultSettings()])
      .then(async ([loadedSettings, loadedUpdate, loadedDefaults]) => {
        settings = loadedSettings;
        update = loadedUpdate;
        defaults = loadedDefaults;
        await settingsWindowReady();
      })
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

    const unlistenUpdate = onUpdateProgress((p) => {
      updateProgress = p;
      if (p.phase === "installing") {
        updateMessage = "Wird installiert…";
      } else if (p.phase === "done") {
        updateMessage = "Installiert — TippIT neu starten.";
        updateBusy = false;
      }
    });

    return () => {
      stopTheme();
      unlisten.then((stop) => stop());
      unlistenUpdate.then((stop) => stop());
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

  const RETENTIONS = [
    { label: "Nie", value: 0 },
    { label: "Nach 7 Tagen", value: 7 },
    { label: "Nach 30 Tagen", value: 30 },
    { label: "Nach 90 Tagen", value: 90 },
    { label: "Nach einem Jahr", value: 365 },
  ] as const;

  function addExcluded() {
    const name = excludeInput.trim();
    if (!settings || name === "") {
      return;
    }
    const list = settings.history.excluded_apps;
    if (!list.some((e) => e.toLowerCase() === name.toLowerCase())) {
      settings.history.excluded_apps = [...list, name];
      save();
    }
    excludeInput = "";
  }

  function removeExcluded(name: string) {
    if (!settings) {
      return;
    }
    settings.history.excluded_apps = settings.history.excluded_apps.filter(
      (e) => e !== name
    );
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

  async function runExport() {
    if (backupPassword.length < 8) {
      backupError = "Das Passwort muss mindestens 8 Zeichen haben.";
      return;
    }
    backupBusy = true;
    backupError = "";
    backupMessage = "";
    try {
      const count = await exportHistory(backupPassword);
      if (count !== null) {
        backupMessage = `${count} Einträge gesichert.`;
        backupPassword = "";
      }
    } catch (e) {
      backupError = String(e);
    } finally {
      backupBusy = false;
    }
  }

  async function runImport() {
    if (backupPassword.length < 8) {
      backupError = "Bitte das Passwort der Sicherung eingeben.";
      return;
    }
    backupBusy = true;
    backupError = "";
    backupMessage = "";
    try {
      const report: ImportReport | null = await importHistory(backupPassword);
      if (report) {
        backupMessage =
          `${report.imported} Einträge übernommen` +
          (report.skipped > 0 ? `, ${report.skipped} bereits vorhanden.` : ".");
        backupPassword = "";
      }
    } catch (e) {
      backupError = String(e);
    } finally {
      backupBusy = false;
    }
  }

  function openFolder() {
    openDataDir().catch(() => {
      // Rust-Log
    });
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
    updateProgress = null;
    updateMessage = "Update wird geladen…";
    try {
      await installUpdate();
    } catch (e) {
      updateMessage = `Update fehlgeschlagen: ${String(e)}`;
      updateProgress = null;
      updateBusy = false;
    }
  }

  /** Beschriftung des Update-Knopfs — zeigt während des Ladens den Fortschritt. */
  const updateLabel = $derived.by(() => {
    if (updateProgress?.phase === "installing") {
      return "Wird installiert…";
    }
    if (updateBusy && updateProgress?.phase === "downloading") {
      return updatePercent === null ? "Lädt…" : `Lädt … ${updatePercent} %`;
    }
    if (updateBusy) {
      return "Lädt…";
    }
    return update ? `Update auf ${update.version}` : "Nach Updates suchen";
  });

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
      // ESC ist überall der Notausstieg: liegt nichts mehr obendrauf, schließt
      // er das Fenster.
      if (event.key === "Escape") {
        getCurrentWindow().close();
      }
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
    <!-- Hero: die zwei Hotkeys SIND die App — Klick auf die Keycaps nimmt neu auf. -->
    <header class="hero">
      {#each HERO_HOTKEYS as hk (hk.key)}
        <button
          class="hk"
          onclick={() =>
            (capturing = capturing === hk.key ? null : hk.key)}
          title="Klicken und neue Tasten drücken (Esc bricht ab)"
          type="button"
          class:recording={capturing === hk.key}
        >
          <span class="caps">
            {#if capturing === hk.key}
              <span class="rec">Tasten drücken…</span>
            {:else}
              {#each formatHotkey(settings.hotkeys[hk.key]).split(" + ") as cap, i (i)}
                {#if i > 0}
                  <span class="plus">+</span>
                {/if}
                <kbd class="cap">{cap}</kbd>
              {/each}
            {/if}
          </span>
          <span class="hk-label" class:mark={q !== "" && hit(hk.kw)}>
            {hk.label}
          </span>
        </button>
      {/each}
      <!-- Fest ESC (kein Setting): nur während eines Tipp-Vorgangs registriert. -->
      <div class="hk static">
        <span class="caps"><kbd class="cap">Esc</kbd></span>
        <span class="hk-label" class:mark={q !== "" && hit("hkEsc")}>
          Tippen abbrechen
        </span>
      </div>
    </header>

    <!-- Tabs + Suche: Suche flacht alle Tabs zu einer Trefferliste ab. -->
    <nav class="tabbar">
      <div class="tabs" class:dim={q !== ""}>
        {#each TABS as tab (tab.id)}
          <button
            class="tab"
            onclick={() => (activeTab = tab.id)}
            type="button"
            class:active={activeTab === tab.id && q === ""}
          >
            <Icon name={tab.icon} size={14} />
            {tab.label}
          </button>
        {/each}
      </div>
      <div class="search">
        <Icon name="search" size={14} />
        <input placeholder="Suchen…" spellcheck="false" bind:value={navQuery}>
      </div>
    </nav>

    <div class="pane" class:searching={q !== ""}>
      <!-- Allgemein -->
      {#if showSection(["sounds", "theme"], "allgemein")}
        {#if q !== ""}
          <div class="glabel">Allgemein</div>
        {/if}
        {#if show("sounds", "allgemein")}
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
        {#if show("theme", "allgemein")}
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
      {/if}

      <!-- Tippen -->
      {#if showSection(["preDelay", "typeMode", "charDelay", "trim"], "tippen")}
        {#if q !== ""}
          <div class="glabel">Tippen</div>
        {/if}
        {#if show("preDelay", "tippen")}
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
        {#if show("typeMode", "tippen")}
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
        {#if show("charDelay", "tippen") && settings.typing.mode === "per_char"}
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
        {#if show("trim", "tippen")}
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
      {/if}

      <!-- Historie -->
      {#if showSection(["maxEntries", "winScale", "capImages", "capFiles", "clearHistory"], "historie")}
        {#if q !== ""}
          <div class="glabel">Historie</div>
        {/if}
        {#if show("maxEntries", "historie")}
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
        {#if show("winScale", "historie")}
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
        {#if show("capImages", "historie")}
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
        {#if show("capFiles", "historie")}
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
        {#if show("capHtml", "historie")}
          <label class="row">
            <span class="row-label">
              Formatierung mitspeichern
              <span class="row-hint">
                Farben und Auszeichnungen bleiben beim Einfügen erhalten.
                Getippt wird immer Klartext.
              </span>
            </span>
            <span class="switch">
              <input
                onchange={save}
                type="checkbox"
                bind:checked={settings.history.capture_html}
              >
              <span class="track"></span>
              <span class="knob"></span>
            </span>
          </label>
        {/if}
        {#if show("retention", "historie")}
          <label class="row">
            <span class="row-label">
              Automatisch aufräumen
              <span class="row-hint">
                Ältere Einträge wandern in den Papierkorb und bleiben dort 30
                Tage. Angepinntes und Bausteine bleiben unberührt.
              </span>
            </span>
            <span class="select">
              <select
                onchange={save}
                bind:value={settings.history.retention_days}
              >
                {#each RETENTIONS as option (option.value)}
                  <option value={option.value}>{option.label}</option>
                {/each}
              </select>
              <Icon name="chevron-down" size={12} />
            </span>
          </label>
        {/if}
        {#if show("excludeApps", "historie")}
          <div class="row">
            <span class="row-label">
              Nichts erfassen aus
              <span class="row-hint">
                Name oder Programmpfad, z. B. „KeePass". Aus diesen Programmen
                landet nichts in der Historie.
              </span>
            </span>
            <span class="grow"></span>
            <input
              class="input"
              onkeydown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  addExcluded();
                }
              }}
              placeholder="Programm hinzufügen"
              spellcheck="false"
              type="text"
              bind:value={excludeInput}
            >
            <button class="btn" onclick={addExcluded} type="button">
              Hinzufügen
            </button>
          </div>
          {#if settings.history.excluded_apps.length > 0}
            <div class="row actions taglist">
              {#each settings.history.excluded_apps as app (app)}
                <button
                  class="chip on"
                  onclick={() => removeExcluded(app)}
                  title="Entfernen"
                  type="button"
                >
                  <Icon name="x" size={12} />{app}
                </button>
              {/each}
            </div>
          {/if}
        {/if}
        {#if show("clearHistory", "historie")}
          <div class="row">
            <span class="row-label">
              Ungepinnte Einträge
              <span class="row-hint">
                Wandern in den Papierkorb — im Historie-Fenster
                wiederherstellbar.
              </span>
            </span>
            <button class="btn danger" onclick={onClearHistory} type="button">
              <Icon name="trash" size={14} />Löschen
            </button>
          </div>
        {/if}
      {/if}

      <!-- Daten: Sicherung und Speicherort -->
      {#if showSection(["backup", "dataDir"], "daten")}
        {#if q !== ""}
          <div class="glabel">Daten</div>
        {/if}
        {#if show("backup", "daten")}
          <div class="row">
            <span class="row-label">
              Sicherung
              <span class="row-hint">
                Die Historie liegt verschlüsselt auf diesem Gerät und lässt sich
                nicht einfach kopieren. Für den Umzug auf einen anderen Rechner
                schreibt der Export eine passwortgeschützte Datei; der Import
                führt sie mit der vorhandenen Historie zusammen.
              </span>
            </span>
          </div>
          <div class="row actions">
            <input
              autocomplete="new-password"
              class="input"
              placeholder="Passwort (mind. 8 Zeichen)"
              type="password"
              bind:value={backupPassword}
            >
            <span class="grow"></span>
            <button
              class="btn"
              disabled={backupBusy}
              onclick={runExport}
              type="button"
            >
              <Icon name="save" size={14} />Exportieren
            </button>
            <button
              class="btn"
              disabled={backupBusy}
              onclick={runImport}
              type="button"
            >
              <Icon name="download" size={14} />Importieren
            </button>
          </div>
          {#if backupBusy}
            <div class="row">
              <span class="row-hint">Schlüssel wird abgeleitet…</span>
            </div>
          {/if}
          {#if backupMessage}
            <div class="row">
              <span class="ok-row"
                ><Icon name="check" size={14} />
                {backupMessage}</span
              >
            </div>
          {/if}
          {#if backupError}
            <div class="row error-row">
              <Icon name="alert" size={14} />
              <span>{backupError}</span>
            </div>
          {/if}
        {/if}
        {#if show("dataDir", "daten")}
          <div class="row">
            <span class="row-label">
              Datenverzeichnis
              <span class="row-hint">
                Datenbank, Schlüssel und Protokolle liegen unter
                <code>~/.labi/tippit/</code>.
              </span>
            </span>
            <button class="btn" onclick={openFolder} type="button">
              <Icon name="external" size={14} />Öffnen
            </button>
          </div>
        {/if}
      {/if}

      {#if noMatch}
        <p class="noresults">Keine Treffer für „{navQuery}"</p>
      {/if}
    </div>

    <!-- Fußzeile: Version, Update und Hilfe — immer da, nie im Weg. -->
    <footer>
      <span class="ver" class:mark={q !== "" && hit("version")}>
        TippIT {appVersion ? `v${appVersion}` : ""}
      </span>
      <button
        class="whatsnew"
        onclick={() => (changelogOpen = true)}
        type="button"
      >
        Was ist neu?
      </button>
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
      <span class="grow"></span>
      <button
        class="btn footbtn"
        disabled={updateBusy}
        onclick={update ? startUpdate : checkUpdate}
        title={updateMessage || undefined}
        type="button"
        style:--p="{updateBusy ? (updatePercent ?? 0) : 0}%"
        class:loading={updateBusy}
        class:primary={Boolean(update)}
      >
        <Icon name="download" size={13} />
        {updateLabel}
      </button>
      <button
        aria-label="Hilfe"
        class="iconbtn"
        onclick={() => (helpOpen = true)}
        title="Hilfe"
        type="button"
      >
        <Icon name="help" size={15} />
      </button>
    </footer>
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
        <div class="modal-body hlp-body">
          <div class="hlp-cols">
            <section>
              <div class="hlp-group">Überall</div>
              {#each [[formatHotkey(settings.hotkeys.paste), "Zwischenablage tippen"], [formatHotkey(settings.hotkeys.history), "Historie öffnen"], ["Esc", "Tippen abbrechen"]] as [keys, what] (what)}
                <div class="hlp-row">
                  <span>{what}</span>
                  <kbd class="fixed-key">{keys}</kbd>
                </div>
              {/each}
            </section>
            <section>
              <div class="hlp-group">In der Historie</div>
              {#each [["↑ ↓", "Eintrag wählen"], ["Enter", "Tippen"], ["Doppelklick", "Einfügen"], [`${primaryModifierLabel}+Doppelklick`, "Zeichenweise tippen"], ["⇧+Enter", "Öffnen / Text extrahieren"], [`${primaryModifierLabel}+P`, "Anpinnen"], [`${primaryModifierLabel}+Entf`, "Löschen"], ["Tab", "Filter wechseln"], ["Esc", "Schließen"]] as [keys, what] (keys)}
                <div class="hlp-row">
                  <span>{what}</span>
                  <kbd class="fixed-key">{keys}</kbd>
                </div>
              {/each}
            </section>
          </div>
          <div class="hlp-foot">
            <p class="hlp-text">
              {#if isMacOS}
                Nichts passiert? Systemeinstellungen → Datenschutz &amp;
                Sicherheit → Bedienungshilfen → TippIT erlauben.
              {:else}
                Nichts passiert? In Fenstern mit Administratorrechten kann
                TippIT nur tippen, wenn es selbst mit Administratorrechten
                läuft.
              {/if}
            </p>
            <p class="hlp-text">
              Logs:
              <button class="pathlink" onclick={openFolder} type="button">
                <code>~/.labi/tippit/</code>
              </button>
            </p>
          </div>
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
          {#each CHANGELOG as release, i (release.title)}
            <details class="log-release" open={i === 0}>
              <summary>
                <Icon name="chevron-down" size={12} />
                <span class="log-ver">{releaseName(release.title)}</span>
                <span class="log-date">{releaseDate(release.title)}</span>
              </summary>
              {#each release.intro as line (line)}
                <p class="log-intro">{line}</p>
              {/each}
              {#each release.groups as group (group.title)}
                <div class="log-group tone-{logTone(group.title)}">
                  <span class="log-dot"></span>{group.title}
                </div>
                <ul class="tone-{logTone(group.title)}">
                  {#each group.items as item (item)}
                    <li>{item}</li>
                  {/each}
                </ul>
              {/each}
            </details>
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
  .grow {
    flex: 1;
  }

  /* ---- Hero: Hotkeys als Keycaps ---- */
  .hero {
    display: flex;
    flex: none;
    gap: 8px;
    align-items: stretch;
    padding: 18px 16px 14px;
    background: var(--bg-sunken);
    border-bottom: 1px solid var(--border);
  }
  .hk {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: 10px;
    align-items: center;
    justify-content: center;
    min-width: 0;
    padding: 14px 8px 12px;
    cursor: pointer;
    background: transparent;
    border: 0;
    border-radius: var(--r-lg);
    transition: background var(--t-fast) linear;
  }
  .hk:hover {
    background: var(--row-hover);
  }
  .hk:focus-visible {
    outline: none;
    box-shadow: var(--shadow-focus);
  }
  .hk.static {
    flex: 0.6;
    cursor: default;
  }
  .hk.static:hover {
    background: transparent;
  }
  .hk.recording {
    background: var(--accent-soft);
  }
  .caps {
    display: flex;
    gap: 5px;
    align-items: center;
    height: 32px;
  }
  .cap {
    display: grid;
    place-items: center;
    min-width: 32px;
    height: 32px;
    padding: 0 8px;
    font: 600 var(--fs-label) / 1 var(--font-ui);
    color: var(--fg);
    background: var(--bg-strong);
    border-radius: var(--r-md);
    box-shadow: inset 0 -2px 0 var(--border);
  }
  .plus {
    font: 500 var(--fs-micro) / 1 var(--font-ui);
    color: var(--fg-dim);
  }
  .rec {
    font: 500 var(--fs-control) / 1 var(--font-ui);
    color: var(--accent-text);
    animation: pulse 1.1s ease-in-out infinite;
  }
  @keyframes pulse {
    50% {
      opacity: 0.4;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .rec {
      animation: none;
    }
  }
  .hk-label {
    font: 450 var(--fs-meta) / 1 var(--font-ui);
    color: var(--fg-muted);
    white-space: nowrap;
  }
  .mark {
    padding: 2px 5px;
    margin: -2px -5px;
    background: var(--mark);
    border-radius: var(--r-sm);
  }

  /* ---- Tabbar + Suche ---- */
  .tabbar {
    display: flex;
    flex: none;
    gap: 10px;
    align-items: center;
    padding: 8px 12px;
    border-bottom: 1px solid var(--border);
  }
  .tabs {
    display: flex;
    flex: 1;
    gap: 2px;
    min-width: 0;
    transition: opacity var(--t-fast) linear;
  }
  .tabs.dim {
    pointer-events: none;
    opacity: 0.4;
  }
  .tab {
    display: inline-flex;
    gap: 7px;
    align-items: center;
    height: 30px;
    padding: 0 12px;
    font: 500 var(--fs-label) / 1 var(--font-ui);
    color: var(--fg-muted);
    cursor: pointer;
    background: transparent;
    border: 0;
    border-radius: var(--r-md);
    transition:
      background var(--t-fast) linear,
      color var(--t-fast) linear;
  }
  .tab:hover {
    color: var(--fg);
    background: var(--row-hover);
  }
  .tab.active {
    color: var(--fg);
    background: var(--bg-raised);
  }
  .tab:focus-visible {
    outline: none;
    box-shadow: var(--shadow-focus);
  }
  .search {
    display: flex;
    flex: none;
    gap: 6px;
    align-items: center;
    width: 170px;
    height: 30px;
    padding: 0 10px;
    color: var(--fg-dim);
    background: var(--bg-raised);
    border-radius: var(--r-md);
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
  .search:focus-within {
    box-shadow: inset 0 0 0 1px var(--border-focus);
  }

  /* ---- Pane (Tab-Inhalt bzw. Suchtreffer) ---- */
  .pane {
    flex: 1;
    min-height: 0;
    padding-bottom: 4px;
    overflow-y: auto;
  }
  .glabel {
    padding: 14px 16px 4px;
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
    min-height: 42px;
    padding: 2px 16px;
  }
  .row + .row {
    border-top: 1px solid var(--border-soft);
  }
  .row-label {
    flex: none;
    font: 450 var(--fs-label) / 1.35 var(--font-ui);
    color: var(--fg);
  }
  /* Suchmodus: sichtbare Zeilen SIND die Treffer — Labels markieren. */
  .pane.searching .row-label {
    padding: 2px 5px;
    margin: -2px -5px;
    background: var(--mark);
    border-radius: var(--r-sm);
  }
  .row.actions {
    flex-wrap: wrap;
    justify-content: flex-start;
    padding-top: 8px;
    padding-bottom: 8px;
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
  .error-row {
    color: var(--danger);
  }
  .error-row span {
    font-size: var(--fs-control);
    color: var(--danger);
  }

  /* ---- Geräteliste ---- */

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

  /* ---- Badge (Sync-Status) ---- */

  /* ---- Details / Erweitert ---- */
  details > summary {
    cursor: pointer;
    list-style: none;
  }
  details > summary::-webkit-details-marker {
    display: none;
  }

  .noresults {
    padding-top: 28%;
    margin: 0;
    font-size: var(--fs-control);
    color: var(--fg-dim);
    text-align: center;
  }

  /* ---- Fußzeile ---- */
  footer {
    display: flex;
    flex: none;
    gap: 10px;
    align-items: center;
    height: 40px;
    padding: 0 10px 0 16px;
    border-top: 1px solid var(--border);
  }
  .ver {
    font: 600 var(--fs-micro) / 1 var(--font-ui);
    color: var(--fg-muted);
    white-space: nowrap;
  }
  .whatsnew {
    padding: 0;
    font-size: var(--fs-micro);
    color: var(--accent-text);
    white-space: nowrap;
    cursor: pointer;
    background: transparent;
    border: 0;
  }
  .whatsnew:hover {
    text-decoration: underline;
  }
  .savebadge {
    display: inline-flex;
    gap: 5px;
    align-items: center;
    height: 20px;
    padding: 0 8px;
    font: 500 var(--fs-micro) / 1 var(--font-ui);
    white-space: nowrap;
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
  .footbtn {
    height: 26px;
    white-space: nowrap;
  }

  /* ---- Feste Tastenanzeige (Hilfe-Overlay) ---- */
  .fixed-key {
    padding: 3px 8px;
    font: 500 var(--fs-micro) / 1 var(--font-ui);
    color: var(--fg-muted);
    white-space: nowrap;
    background: var(--bg-strong);
    border-radius: var(--r-sm);
  }

  /* ---- Overlays (Hilfe, Changelog) ---- */
  /* Overlays füllen das ganze Fenster — Rahmen, Radius und Schatten entfallen,
     weil nichts mehr dahinter sichtbar ist. */
  .modal-backdrop {
    position: fixed;
    inset: 0;
    z-index: 10;
    display: flex;
    background: var(--bg-base);
  }
  .modal {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
    overflow: hidden;
    background: var(--bg-base);
  }
  .modal-head {
    display: flex;
    flex: none;
    align-items: center;
    justify-content: space-between;
    padding: 10px 14px;
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
    padding: 4px 14px 14px;
    overflow-y: auto;
    user-select: text;
  }

  /* ---- Hilfe-Inhalt: zwei Spalten, solange die Breite reicht ---- */
  .hlp-cols {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(230px, 1fr));
    gap: 0 24px;
    align-items: start;
  }
  .hlp-group {
    margin: 12px 0 2px;
    font: 600 var(--fs-micro) / 1 var(--font-ui);
    color: var(--fg-dim);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  /* Fußnoten kleben am unteren Rand der Hilfe-Seite. */
  .hlp-body {
    display: flex;
    flex-direction: column;
  }
  .hlp-foot {
    padding-top: 20px;
    margin-top: auto;
  }
  /* Fortschritt läuft als Fläche durch den Knopf selbst — keine zweite Zeile
     in der ohnehin schmalen Fußleiste. */
  .footbtn.loading {
    background:
      linear-gradient(var(--accent), var(--accent)) left / var(--p) 100%
      no-repeat,
      var(--bg-raised);
    transition: background-size var(--t-base) linear;
  }
  .pathlink code {
    color: var(--accent-text);
  }
  .pathlink {
    padding: 0;
    font: inherit;
    color: var(--accent-text);
    cursor: pointer;
    background: none;
    border: 0;
  }
  .pathlink:hover code {
    text-decoration: underline;
  }
  .hlp-text {
    margin: 0 0 6px;
    font-size: var(--fs-meta);
    line-height: 1.5;
    color: var(--fg-muted);
  }
  .hlp-text code {
    font-family: var(--font-mono);
    font-size: var(--fs-meta);
    color: var(--fg-muted);
  }
  .hlp-row {
    display: flex;
    gap: 12px;
    align-items: center;
    justify-content: space-between;
    min-height: 24px;
    font-size: var(--fs-control);
    color: var(--fg-body);
  }
  .hlp-row + .hlp-row {
    border-top: 1px solid var(--border-soft);
  }

  /* ---- Changelog-Inhalt: pro Release aufklappbar, neuester offen ---- */
  .log-release + .log-release {
    border-top: 1px solid var(--border-soft);
  }
  .log-release summary {
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 7px 0;
    color: var(--fg);
    cursor: pointer;
    list-style: none;
  }
  .log-release summary::-webkit-details-marker {
    display: none;
  }
  /* Chevron zeigt zu, solange der Block eingeklappt ist. */
  .log-release summary :global(svg) {
    flex: none;
    color: var(--fg-dim);
    transform: rotate(-90deg);
    transition: transform var(--t-fast) ease;
  }
  .log-release[open] summary :global(svg) {
    transform: rotate(0deg);
  }
  .log-ver {
    font: 600 var(--fs-label) / 1 var(--font-ui);
  }
  .log-date {
    margin-left: auto;
    font-size: var(--fs-micro);
    color: var(--fg-muted);
  }
  .log-intro {
    margin: 2px 0 4px 20px;
    font-size: var(--fs-control);
    color: var(--fg-body);
  }
  .log-group {
    display: flex;
    gap: var(--s-2);
    align-items: center;
    margin: 10px 0 2px 20px;
    font: 600 var(--fs-micro) / 1 var(--font-ui);
    color: var(--fg-dim);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  /* Farbe trägt die Bedeutung der Gruppe — Punkt und Listenlinie teilen sie. */
  .log-dot {
    width: 6px;
    height: 6px;
    background: currentcolor;
    border-radius: var(--r-full);
  }
  .tone-add {
    color: var(--success);
  }
  .tone-change {
    color: var(--accent-text);
  }
  .tone-remove {
    color: var(--danger);
  }
  .tone-fix {
    color: var(--warn);
  }
  .log-release ul.tone-add {
    border-left: 2px solid var(--success-soft);
  }
  .log-release ul.tone-change {
    border-left: 2px solid var(--accent-soft);
  }
  .log-release ul.tone-remove {
    border-left: 2px solid var(--danger-soft);
  }
  .log-release ul.tone-fix {
    border-left: 2px solid var(--warn-soft);
  }
  .log-release ul {
    padding-left: 16px;
    margin: 2px 0 8px 20px;
  }
  .log-release li {
    margin: 2px 0;
    font-size: var(--fs-control);
    line-height: 1.45;
    color: var(--fg-body);
  }
  /* Erklärzeile unter einer Einstellungs-Beschriftung. */
  .row-hint {
    display: block;
    max-width: 46ch;
    margin-top: 2px;
    font-size: var(--fs-micro);
    font-weight: 400;
    line-height: 1.45;
    color: var(--fg-dim);
  }
  .row-hint code {
    font-family: var(--font-mono);
  }
  .ok-row {
    display: inline-flex;
    gap: var(--s-2);
    align-items: center;
    font-size: var(--fs-micro);
    color: var(--success);
  }
  .taglist {
    flex-wrap: wrap;
    justify-content: flex-start;
  }
</style>
