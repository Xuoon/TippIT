<script lang="ts">
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
  import { initTheme, setThemeMode } from "$lib/theme";
  import "$lib/theme.css";

  let settings = $state<Settings | null>(null);
  let defaults = $state<Settings | null>(null);
  let saveState = $state<"idle" | "saved" | "error">("idle");
  let capturing = $state<"cancel" | "history" | "paste" | null>(null);
  let sync = $state<SyncStatus | null>(null);
  let joinCode = $state("");
  let syncBusy = $state(false);
  let syncError = $state("");
  let codeCopied = $state(false);
  let update = $state<UpdateMetadata | null>(null);
  let updateBusy = $state(false);
  let updateMessage = $state("");

  let navQuery = $state("");
  let activeSection = $state("allgemein");
  let scrollEl = $state<HTMLDivElement | undefined>();

  const SECTIONS = [
    "allgemein",
    "hotkeys",
    "tippen",
    "historie",
    "sync",
    "updates",
  ] as const;

  const NAV = [
    { icon: "sliders", id: "allgemein", label: "Allgemein" },
    { icon: "keyboard", id: "hotkeys", label: "Hotkeys" },
    { icon: "cursor-text", id: "tippen", label: "Tippen" },
    { icon: "clock", id: "historie", label: "Historie" },
    { icon: "sync", id: "sync", label: "Synchronisierung" },
    { icon: "download", id: "updates", label: "Updates" },
  ] as const;

  const HK_KW: Record<string, string> = {
    cancel: "hkCancel",
    history: "hkHistory",
    paste: "hkPaste",
  };

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
      "hotkey tastenkürzel zwischenablage tippen einfügen strg e shortcut",
    hkHistory:
      "hotkey tastenkürzel historie öffnen verlauf strg shift e shortcut",
    hkCancel: "hotkey abbrechen stopp escape tippen anhalten",
    preDelay: "startverzögerung verzögerung delay wartezeit vorlauf tippen",
    typeMode:
      "modus zeichenweise auf einmal bulk per char tippen geschwindigkeit",
    charDelay:
      "zeichenabstand tempo geschwindigkeit delay tippen millisekunden",
    trim: "leerraum entfernen trim whitespace leerzeichen kürzen",
    maxEntries: "maximale einträge anzahl limit historie größe aufbewahren",
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
    updStatus: "update status version prüfen aktuell",
    updActions: "update installieren aktualisieren suchen prüfen version",
  };

  const q = $derived(navQuery.trim().toLowerCase());
  const hit = (key: string) => q === "" || (KEYWORDS[key] ?? "").includes(q);
  const sectionHit = (keys: string[]) => keys.some(hit);
  const noMatch = $derived(q !== "" && !Object.keys(KEYWORDS).some(hit));

  onMount(() => {
    const stopTheme = initTheme();
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
  const POLICIES: { key: PolicyKey; label: string; kw: string }[] = [
    { key: "allow_mobile_data", label: "Mobilfunk", kw: "polMobile" },
    { key: "allow_energy_saver", label: "Energiesparmodus", kw: "polEnergy" },
    { key: "allow_data_saver", label: "Datensparmodus", kw: "polData" },
  ];

  function togglePolicy(key: PolicyKey) {
    if (!settings) {
      return;
    }
    settings.sync[key] = !settings.sync[key];
    save();
  }

  function onScroll() {
    if (!scrollEl || q !== "") {
      return;
    }
    const mark = scrollEl.scrollTop + 64;
    let found: string = SECTIONS[0];
    for (const id of SECTIONS) {
      const el = document.getElementById(id);
      if (el && el.offsetTop <= mark) {
        found = id;
      }
    }
    activeSection = found;
  }

  function goTo(id: string) {
    activeSection = id;
    document.getElementById(id)?.scrollIntoView({ block: "start" });
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

  const F_KEY_PATTERN = /^F\d{1,2}$/;
  function keyFromCode(code: string): string | null {
    if (code.startsWith("Key")) {
      return code.slice(3).toLowerCase();
    }
    if (code.startsWith("Digit")) {
      return code.slice(5);
    }
    if (F_KEY_PATTERN.test(code)) {
      return code.toLowerCase();
    }
    return null;
  }

  function captureHotkey(event: KeyboardEvent) {
    if (!(capturing && settings)) {
      return;
    }
    event.preventDefault();
    if (event.key === "Escape") {
      capturing = null;
      return;
    }
    const key = keyFromCode(event.code);
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

  function fmtHotkey(hotkey: string): string {
    return hotkey.toUpperCase().replace("CTRL", "STRG").replaceAll("+", " + ");
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
    <aside class="sidebar">
      <div class="nav-search">
        <Icon name="search" size={16} />
        <input
          placeholder="Einstellung suchen…"
          spellcheck="false"
          bind:value={navQuery}
        >
      </div>
      <nav class="nav" class:dim={q !== ""}>
        {#each NAV as item (item.id)}
          <button
            class="nav-item"
            onclick={() => goTo(item.id)}
            title={item.label}
            type="button"
            class:active={activeSection === item.id}
          >
            <Icon name={item.icon} size={16} />
            <span class="nav-label">{item.label}</span>
            {#if item.id === "sync"}
              <span
                class="nav-dot"
                class:active={syncState === "active"}
                class:off={syncState === "off"}
                class:paused={syncState === "paused"}
              ></span>
            {:else if item.id === "updates" && update}
              <span class="nav-dot accent"></span>
            {/if}
          </button>
        {/each}
      </nav>
      <div class="credit">TippIT · Sven Labitzki</div>
    </aside>

    <div class="hair"></div>

    <div class="content">
      <span
        class="savebadge"
        class:error={saveState === "error"}
        class:on={saveState !== "idle"}
        class:saved={saveState === "saved"}
      >
        {#if saveState === "saved"}
          <Icon name="check" size={12} />Gespeichert
        {:else if saveState === "error"}
          <Icon name="alert" size={12} />Fehler beim Speichern
        {/if}
      </span>

      <div class="scroll" onscroll={onScroll} bind:this={scrollEl}>
        <div class="inner" class:searching={q !== ""}>
          <!-- Sektion 1 — Allgemein -->
          {#if sectionHit(["sounds", "theme"])}
            <section id="allgemein">
              <h2>Allgemein</h2>
              <div class="card">
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
              </div>
            </section>
          {/if}

          <!-- Sektion 2 — Hotkeys -->
          {#if sectionHit(["hkPaste", "hkHistory", "hkCancel"])}
            <section id="hotkeys">
              <h2>Hotkeys</h2>
              <div class="card">
                {#each [["paste", "Zwischenablage tippen"], ["history", "Historie öffnen"], ["cancel", "Tippen abbrechen"]] as [ key, label ] (key)}
                  {#if hit(HK_KW[key])}
                    <div class="row">
                      <span class="row-label">{label}</span>
                      <button
                        class="btn hotkey"
                        onclick={() =>
                          (capturing =
                            capturing === key
                              ? null
                              : (key as "cancel" | "history" | "paste"))}
                        type="button"
                        class:recording={capturing === key}
                      >
                        {capturing === key
                          ? "Tasten drücken…"
                          : fmtHotkey(
                              settings.hotkeys[
                                key as "cancel" | "history" | "paste"
                              ]
                            )}
                      </button>
                    </div>
                  {/if}
                {/each}
              </div>
            </section>
          {/if}

          <!-- Sektion 3 — Tippen -->
          {#if sectionHit(["preDelay", "typeMode", "charDelay", "trim"])}
            <section id="tippen">
              <h2>Tippen</h2>
              <div class="card">
                {#if hit("preDelay")}
                  <label class="row-stack">
                    <span class="top">
                      <span class="row-label">Startverzögerung</span>
                      <output>{settings.typing.pre_delay_ms} ms</output>
                    </span>
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
                  <label class="row-stack">
                    <span class="top">
                      <span class="row-label">Zeichenabstand</span>
                      <output>{settings.typing.char_delay_ms} ms</output>
                    </span>
                    <input
                      max="100"
                      min="1"
                      onchange={save}
                      ondblclick={resetCharDelay}
                      title="Doppelklick: Standard"
                      type="range"
                      bind:value={settings.typing.char_delay_ms}
                    >
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
              </div>
            </section>
          {/if}

          <!-- Sektion 4 — Historie -->
          {#if sectionHit(["maxEntries", "capImages", "capFiles", "clearHistory"])}
            <section id="historie">
              <h2>Historie</h2>
              <div class="card">
                {#if hit("maxEntries")}
                  <label class="row-stack">
                    <span class="top">
                      <span class="row-label">Maximale Einträge</span>
                      <output>{settings.history.max_entries}</output>
                    </span>
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
                    <button
                      class="btn danger"
                      onclick={onClearHistory}
                      type="button"
                    >
                      <Icon name="trash" size={14} />Löschen
                    </button>
                  </div>
                {/if}
              </div>
            </section>
          {/if}

          <!-- Sektion 5 — Synchronisierung -->
          {#if sectionHit(["syncConn", "syncText", "syncSettings", "syncImages", "syncInterval", "polMobile", "polEnergy", "polData", "syncUrl"])}
            <section id="sync">
              <div class="sync-title">
                <h2>Synchronisierung</h2>
                <span
                  class="badge syncbadge"
                  class:active={syncState === "active"}
                  class:off={syncState === "off"}
                  class:paused={syncState === "paused"}
                >
                  <span class="dot"></span>{syncStateLabel}
                </span>
              </div>
              <div class="card">
                {#if hit("syncConn")}
                  <div class="subhead">Verbindung</div>
                  {#if sync?.active}
                    <div class="row-actions">
                      <button
                        class="btn"
                        disabled={syncBusy}
                        onclick={copyCode}
                        type="button"
                      >
                        <Icon name="copy" size={14} />
                        {codeCopied
                          ? "Kopiert"
                          : "Code kopieren"}
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
                    <div class="row-actions">
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

                {#if sectionHit(["syncText", "syncSettings", "syncImages", "syncInterval"])}
                  <div class="subhead">Umfang &amp; Zeitplan</div>
                  {#if hit("syncText")}
                    <label class="row">
                      <span class="row-label">Text &amp; Dateipfade</span>
                      <span class="switch">
                        <input
                          onchange={save}
                          type="checkbox"
                          bind:checked={settings.sync.sync_text}
                        >
                        <span class="track"></span>
                        <span class="knob"></span>
                      </span>
                    </label>
                  {/if}
                  {#if hit("syncSettings")}
                    <label class="row">
                      <span class="row-label">Einstellungen</span>
                      <span class="switch">
                        <input
                          onchange={save}
                          type="checkbox"
                          bind:checked={settings.sync.sync_settings}
                        >
                        <span class="track"></span>
                        <span class="knob"></span>
                      </span>
                    </label>
                  {/if}
                  {#if hit("syncImages")}
                    <label class="row">
                      <span class="row-label"
                        >Bilder bis
                        {Math.round(
                          settings.sync.image_max_bytes / 1024
                        )}
                        KB</span
                      >
                      <span class="switch">
                        <input
                          onchange={save}
                          type="checkbox"
                          bind:checked={settings.sync.sync_images}
                        >
                        <span class="track"></span>
                        <span class="knob"></span>
                      </span>
                    </label>
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
                {/if}

                {#if sectionHit(["polMobile", "polEnergy", "polData"])}
                  <div class="subhead">Systemrichtlinien</div>
                  <p class="subhelp">
                    Sync läuft in diesen Modi nur, wenn erlaubt.
                  </p>
                  <div class="row-actions chips">
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
                    <label class="row-stack">
                      <span class="top">
                        <span class="row-label">Server-URL</span>
                      </span>
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
              </div>
            </section>
          {/if}

          <!-- Sektion 6 — Updates -->
          {#if sectionHit(["updStatus", "updActions"])}
            <section id="updates">
              <h2>Updates</h2>
              <div class="card">
                {#if hit("updStatus")}
                  <div class="row">
                    <span class="row-label">Status</span>
                    <span class="upd-status"
                      >{updateMessage ||
                        (update
                          ? `Version ${update.version} ist verfügbar.`
                          : "Automatische Prüfung beim Start ist aktiv.")}</span
                    >
                  </div>
                {/if}
                {#if hit("updActions")}
                  <div class="row-actions">
                    {#if update}
                      <button
                        class="btn primary"
                        disabled={updateBusy}
                        onclick={startUpdate}
                        type="button"
                      >
                        <Icon name="download" size={14} />Jetzt aktualisieren
                      </button>
                    {/if}
                    <button
                      class="btn"
                      disabled={updateBusy}
                      onclick={checkUpdate}
                      type="button"
                    >
                      Nach Updates suchen
                    </button>
                  </div>
                {/if}
              </div>
            </section>
          {/if}

          {#if noMatch}
            <p class="noresults">Keine Treffer für „{navQuery}"</p>
          {/if}
        </div>
      </div>
    </div>
  </main>
{/if}

<style>
  :global(body) {
    margin: 0;
    overflow: hidden;
    font-family: var(--font-ui);
    user-select: none;
  }

  main {
    display: grid;
    grid-template-columns: 220px 1px 1fr;
    height: 100vh;
    overflow: hidden;
    background: var(--bg-sunken);
  }

  /* ---- Sidebar ---- */
  .sidebar {
    display: flex;
    flex-direction: column;
    min-height: 0;
    padding: 12px 10px 10px;
    overflow: hidden;
    background: var(--bg-base);
  }
  .nav-search {
    display: flex;
    gap: 8px;
    align-items: center;
    height: 30px;
    padding: 0 10px;
    margin-bottom: 12px;
    color: var(--fg-dim);
    background: var(--bg-raised);
    border-radius: var(--r-md);
  }
  .nav-search input {
    flex: 1;
    min-width: 0;
    font-size: var(--fs-control);
    color: var(--fg);
    outline: none;
    background: transparent;
    border: 0;
  }
  .nav-search input::placeholder {
    color: var(--fg-placeholder);
  }
  .nav-search:focus-within {
    box-shadow: inset 0 0 0 1px var(--border-focus);
  }
  .nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
    transition: opacity var(--t-fast) linear;
  }
  .nav.dim {
    pointer-events: none;
    opacity: 0.4;
  }
  .nav-item {
    position: relative;
    display: flex;
    gap: 10px;
    align-items: center;
    width: 100%;
    height: 32px;
    padding: 0 10px;
    font: 450 var(--fs-label) / 1 var(--font-ui);
    color: var(--fg-muted);
    text-align: left;
    cursor: pointer;
    background: transparent;
    border: 0;
    border-radius: var(--r-md);
    transition:
      background var(--t-fast) linear,
      color var(--t-fast) linear;
  }
  .nav-item:hover {
    color: var(--fg-body);
    background: var(--row-hover);
  }
  .nav-item.active {
    color: var(--fg);
    background: var(--bg-raised);
  }
  .nav-label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .nav-dot {
    flex: none;
    width: 6px;
    height: 6px;
    margin-left: auto;
    background: var(--fg-disabled);
    border-radius: 50%;
  }
  .nav-dot.active {
    background: var(--success);
  }
  .nav-dot.paused {
    background: var(--warn);
  }
  .nav-dot.off {
    background: var(--fg-disabled);
  }
  .nav-dot.accent {
    background: var(--accent);
  }
  .credit {
    padding: 10px 10px 2px;
    margin-top: auto;
    font-size: var(--fs-code);
    color: var(--fg-disabled);
    border-top: 1px solid var(--border);
  }

  .hair {
    background: var(--border);
  }

  /* ---- Inhaltsspalte ---- */
  .content {
    position: relative;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    background: var(--bg-sunken);
  }
  .savebadge {
    position: absolute;
    top: 12px;
    right: 20px;
    z-index: 2;
    display: inline-flex;
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

  .scroll {
    position: relative;
    flex: 1;
    min-height: 0;
    padding: 24px 32px 48px;
    overflow-y: auto;
  }
  .inner {
    max-width: 640px;
  }
  section {
    margin-bottom: 28px;
    scroll-margin-top: 16px;
  }

  h2 {
    margin: 0 0 8px 14px;
    font: 600 var(--fs-meta) / 1 var(--font-ui);
    color: var(--fg-dim);
    text-transform: uppercase;
    letter-spacing: 0.3px;
  }
  .sync-title {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin: 0 0 8px 14px;
  }
  .sync-title h2 {
    margin: 0;
  }

  /* ---- Karte ---- */
  .card {
    overflow: hidden;
    background: var(--bg-base);
    border: 0;
    border-radius: var(--r-xl);
  }
  .card > * + * {
    border-top: 1px solid var(--border-soft);
  }
  .subhead {
    padding: 14px 14px 4px;
    font: 600 var(--fs-meta) / 1 var(--font-ui);
    color: var(--fg-dim);
    border-top: 1px solid var(--border);
  }
  .card > *:first-child {
    border-top: 0;
  }
  .card > .subhead + * {
    border-top: 0;
  }
  .subhelp {
    padding: 0 14px 8px;
    margin: 0;
    font-size: var(--fs-meta);
    color: var(--fg-dim);
  }
  .card > .subhelp {
    border-top: 0;
  }

  /* ---- Zeilengrammatik ---- */
  .row {
    display: flex;
    gap: 16px;
    align-items: center;
    justify-content: space-between;
    min-height: 44px;
    padding: 0 14px;
  }
  .row-label {
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
  .row-stack {
    display: block;
    padding: 12px 14px;
  }
  .row-stack .top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 8px;
  }
  .row-stack output {
    font: 400 var(--fs-button) / 1 var(--font-ui);
    font-variant-numeric: tabular-nums;
    color: var(--accent-text);
  }
  .row-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
    min-height: 44px;
    padding: 12px 14px;
  }
  .error-row {
    color: var(--danger);
  }
  .error-row span {
    font-size: var(--fs-control);
    color: var(--danger);
  }
  .upd-status {
    font-size: var(--fs-control);
    color: var(--fg-body);
    text-align: end;
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
    background: #ffffff;
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

  /* ---- Slider ---- */
  /* Doppelklick setzt auf den Auslieferungs-Default zurück (resetXyz-Handler). */
  input[type="range"] {
    width: 100%;
    height: 4px;
    margin: 0;
    accent-color: var(--accent);
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
    width: 100%;
  }

  /* ---- Richtlinien-Chips ---- */
  .row-actions.chips {
    padding-top: 4px;
  }
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

  /* ---- Badge ---- */
  .badge {
    display: inline-flex;
    gap: 6px;
    align-items: center;
    height: 20px;
    padding: 0 8px;
    font: 500 var(--fs-micro) / 1 var(--font-ui);
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
  /* Steht bewusst nach den details-Regeln: die .ic-Regeln müssen in
         aufsteigender Spezifität stehen (noDescendingSpecificity). */
  .nav .nav-item.active :global(.ic) {
    color: var(--accent-text);
  }

  .noresults {
    padding-top: 38%;
    margin: 0;
    font-size: var(--fs-control);
    color: var(--fg-dim);
    text-align: center;
  }

  /* ---- Schmaler Modus ---- */
  @media (max-width: 819px) {
    main {
      grid-template-columns: 56px 1px 1fr;
    }
    .nav-label,
    .nav-search,
    .credit {
      display: none;
    }
    .nav-item {
      justify-content: center;
      padding: 0;
    }
    .nav-dot {
      position: absolute;
      top: 6px;
      right: 6px;
      margin-left: 0;
    }
  }
</style>
