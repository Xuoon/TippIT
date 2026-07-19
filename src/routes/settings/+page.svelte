<script lang="ts">
  import { onMount } from "svelte";
  import {
    checkForUpdate,
    clearHistory,
    getSettings,
    installUpdate,
    onSettingsChanged,
    type PairingInfo,
    pendingUpdate,
    type Settings,
    type SyncStatus,
    setSettings,
    settingsWindowReady,
    syncCopyCode,
    syncCreateGroup,
    syncJoinGroup,
    syncLeaveGroup,
    syncNewCode,
    syncShowPairing,
    syncStatus,
    type UpdateMetadata,
  } from "$lib/api";

  let settings = $state<Settings | null>(null);
  let saveState = $state<"idle" | "saved" | "error">("idle");
  let capturing = $state<"paste" | "history" | null>(null);
  let sync = $state<SyncStatus | null>(null);
  let pairing = $state<PairingInfo | null>(null);
  let joinCode = $state("");
  let syncBusy = $state(false);
  let syncError = $state("");
  let codeCopied = $state(false);
  let update = $state<UpdateMetadata | null>(null);
  let updateBusy = $state(false);
  let updateMessage = $state("");

  const BLOCK_LABELS = {
    data_saver: "Pausiert: Datensparmodus",
    energy_saver: "Pausiert: Energiesparmodus",
    mobile_data: "Pausiert: Mobilfunk",
  } as const;

  const syncStateLabel = $derived.by(() => {
    if (!sync?.active) {
      return "Nicht verbunden";
    }
    return sync.blocked_reason ? BLOCK_LABELS[sync.blocked_reason] : "Aktiv";
  });

  onMount(() => {
    Promise.all([getSettings(), syncStatus(), pendingUpdate()])
      .then(async ([loadedSettings, loadedSync, loadedUpdate]) => {
        settings = loadedSettings;
        sync = loadedSync;
        update = loadedUpdate;
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

    // Richtlinien-Pausen (Energiesparmodus etc.) können sich jederzeit ändern;
    // das Badge folgt der Realität, solange das Fenster sichtbar ist.
    const statusTimer = setInterval(() => {
      if (!document.hidden) {
        syncStatus().then((s) => (sync = s));
      }
    }, 30_000);

    return () => {
      clearInterval(statusTimer);
      unlisten.then((stop) => stop());
    };
  });

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
      pairing = null;
    } catch (e) {
      syncError = String(e);
    } finally {
      syncBusy = false;
    }
  }

  async function togglePairing() {
    if (pairing) {
      pairing = null;
      return;
    }
    try {
      pairing = await syncShowPairing();
    } catch (e) {
      syncError = String(e);
    }
  }

  async function copyCode() {
    await syncCopyCode();
    codeCopied = true;
    setTimeout(() => (codeCopied = false), 1500);
  }

  async function newCode() {
    const warning = sync?.active
      ? "Ein neuer Code trennt dieses Gerät von der aktiven Gruppe. Fortfahren?"
      : "Ein neuer Code ersetzt den bisherigen TippIT-Code. Fortfahren?";
    // biome-ignore lint/suspicious/noAlert: bewusster nativer Bestätigungsdialog
    if (!confirm(warning)) {
      return;
    }
    syncBusy = true;
    try {
      pairing = await syncNewCode();
      sync = await syncStatus();
    } catch (e) {
      syncError = String(e);
    } finally {
      syncBusy = false;
    }
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
    <header>
      <div>
        <h1>Einstellungen</h1>
        <p>Alle Optionen auf einen Blick</p>
      </div>
      <span
        class="save-state"
        class:error={saveState === "error"}
        class:visible={saveState !== "idle"}
      >
        {#if saveState === "saved"}
          Gespeichert
        {:else if saveState === "error"}
          Fehler beim Speichern
        {/if}
      </span>
    </header>

    <div class="grid">
      <section>
        <h2>Allgemein</h2>
        <label class="line"
          ><span
            ><b>Sounds</b><small>Feedback beim Tippen und Kopieren</small></span
          ><input
            onchange={save}
            type="checkbox"
            bind:checked={settings.sounds}
          ></label
        >
        <label class="line"
          ><span><b>Darstellung</b><small>Farbschema der Anwendung</small></span
          ><select onchange={save} bind:value={settings.theme}>
            <option value="system">System</option>
            <option value="dark">Dunkel</option>
            <option value="light">Hell</option>
          </select></label
        >
      </section>

      <section>
        <h2>Hotkeys</h2>
        {#each [["paste", "Zwischenablage tippen"], ["history", "Historie öffnen"]] as [ key, label ] (key)}
          <div class="line">
            <span><b>{label}</b><small>Zum Ändern anklicken</small></span
            ><button
              class="value-button"
              onclick={() => (capturing = capturing === key ? null : key as "paste" | "history")}
              type="button"
              class:recording={capturing === key}
            >
              {capturing === key ? "Tasten drücken…" : fmtHotkey(settings.hotkeys[key as "paste" | "history"])}
            </button>
          </div>
        {/each}
      </section>

      <section>
        <h2>Tippen</h2>
        <label class="stack"
          ><span
            ><b>Startverzögerung</b
            ><output>{settings.typing.pre_delay_ms} ms</output></span
          ><input
            max="3000"
            min="0"
            onchange={save}
            step="100"
            type="range"
            bind:value={settings.typing.pre_delay_ms}
          ></label
        >
        <label class="line"
          ><span><b>Modus</b><small>Zeichenweise ist kompatibler</small></span
          ><select onchange={save} bind:value={settings.typing.mode}>
            <option value="per_char">Zeichenweise</option>
            <option value="bulk">Auf einmal</option>
          </select></label
        >
        {#if settings.typing.mode === "per_char"}
          <label class="stack"
            ><span
              ><b>Zeichenabstand</b
              ><output>{settings.typing.char_delay_ms} ms</output></span
            ><input
              max="100"
              min="1"
              onchange={save}
              type="range"
              bind:value={settings.typing.char_delay_ms}
            ></label
          >
        {/if}
        <label class="line"
          ><span
            ><b>Leerraum entfernen</b><small>Am Anfang und Ende</small></span
          ><input
            onchange={save}
            type="checkbox"
            bind:checked={settings.typing.trim}
          ></label
        >
      </section>

      <section>
        <h2>Historie</h2>
        <label class="stack"
          ><span
            ><b>Maximale Einträge</b
            ><output>{settings.history.max_entries}</output></span
          ><input
            max="5000"
            min="100"
            onchange={save}
            step="100"
            type="range"
            bind:value={settings.history.max_entries}
          ></label
        >
        <label class="line"
          ><span><b>Bilder erfassen</b></span>
          <input
            onchange={save}
            type="checkbox"
            bind:checked={settings.history.capture_images}
          ></label
        >
        <label class="line"
          ><span><b>Dateipfade erfassen</b></span>
          <input
            onchange={save}
            type="checkbox"
            bind:checked={settings.history.capture_files}
          ></label
        >
        <button
          class="text-button danger"
          onclick={onClearHistory}
          type="button"
        >
          Ungepinnte Historie löschen
        </button>
      </section>

      <section class="wide sync-section">
        <div class="section-title">
          <div>
            <h2>Synchronisierung</h2>
            <p>Ende-zu-Ende-verschlüsselt, ohne Benutzerkonto</p>
          </div>
          <span
            class="sync-state"
            class:active={sync?.active && !sync?.blocked_reason}
            class:paused={sync?.active && sync?.blocked_reason}
            >{syncStateLabel}</span
          >
        </div>
        <div class="sync-grid">
          <div>
            <h3>Verbindung</h3>
            <div class="inline-actions">
              <button
                class="text-button accent"
                disabled={syncBusy}
                onclick={copyCode}
                type="button"
              >
                {codeCopied ? "Kopiert" : "Code kopieren"}
              </button><button
                class="text-button"
                disabled={syncBusy}
                onclick={togglePairing}
                type="button"
              >
                {pairing ? "QR ausblenden" : "Code + QR"}
              </button><button
                class="text-button"
                disabled={syncBusy}
                onclick={newCode}
                type="button"
              >
                Neuer Code
              </button>
            </div>
            {#if pairing}
              <div class="pairing">
                <div class="qr">{@html pairing.qr_svg}</div>
                <code>{pairing.code}</code>
              </div>
            {/if}
            {#if !sync?.active}
              <div class="join">
                <input
                  placeholder="TIPPIT-Code"
                  spellcheck="false"
                  type="text"
                  bind:value={joinCode}
                ><button
                  class="text-button accent"
                  disabled={syncBusy || joinCode.length < 20 || !settings.sync.deployment_url}
                  onclick={() => withSync(() => syncJoinGroup(joinCode))}
                  type="button"
                >
                  Beitreten
                </button><button
                  class="text-button"
                  disabled={syncBusy || !settings.sync.deployment_url}
                  onclick={() => withSync(syncCreateGroup)}
                  type="button"
                >
                  Neue Gruppe
                </button>
              </div>
            {:else}
              <button
                class="text-button danger"
                disabled={syncBusy}
                onclick={() => withSync(syncLeaveGroup)}
                type="button"
              >
                Gruppe verlassen
              </button>
            {/if}
            {#if syncError}
              <p class="message error-text">{syncError}</p>
            {/if}
          </div>
          <div>
            <h3>Umfang & Zeitplan</h3>
            <label class="line"
              ><span>Text & Dateipfade</span>
              <input
                onchange={save}
                type="checkbox"
                bind:checked={settings.sync.sync_text}
              ></label
            >
            <label class="line"
              ><span>Einstellungen</span>
              <input
                onchange={save}
                type="checkbox"
                bind:checked={settings.sync.sync_settings}
              ></label
            >
            <label class="line"
              ><span
                >Bilder bis
                {Math.round(settings.sync.image_max_bytes / 1024)}
                KB</span
              ><input
                onchange={save}
                type="checkbox"
                bind:checked={settings.sync.sync_images}
              ></label
            >
            <label class="line"
              ><span>Synchronisieren</span
              ><select
                onchange={save}
                bind:value={settings.sync.interval_minutes}
              >
                <option value={0}>Sofort</option>
                <option value={1}>Jede Minute</option>
                <option value={5}>Alle 5 Minuten</option>
                <option value={15}>Alle 15 Minuten</option>
                <option value={30}>Alle 30 Minuten</option>
                <option value={60}>Stündlich</option>
              </select></label
            >
          </div>
          <div>
            <h3>Systemrichtlinien</h3>
            <label class="line"
              ><span
                ><b>Über Mobilfunk</b
                ><small>WWAN-Verbindungen zulassen</small></span
              ><input
                onchange={save}
                type="checkbox"
                bind:checked={settings.sync.allow_mobile_data}
              ></label
            >
            <label class="line"
              ><span
                ><b>Im Energiesparmodus</b
                ><small>Hintergrund-Sync fortsetzen</small></span
              ><input
                onchange={save}
                type="checkbox"
                bind:checked={settings.sync.allow_energy_saver}
              ></label
            >
            <label class="line"
              ><span
                ><b>Im Datensparmodus</b
                ><small>Windows-Datenlimit ignorieren</small></span
              ><input
                onchange={save}
                type="checkbox"
                bind:checked={settings.sync.allow_data_saver}
              ></label
            >
          </div>
        </div>
        <details>
          <summary>Erweitert</summary>
          <label class="server"
            ><span>Server-URL</span>
            <input
              onchange={save}
              placeholder="https://….convex.cloud"
              type="text"
              bind:value={settings.sync.deployment_url}
            ></label
          >
        </details>
      </section>

      <section class="wide update-section">
        <div>
          <h2>Updates</h2>
          <p>
            {updateMessage ||
              (update
                ? `Version ${update.version} ist verfügbar.`
                : "Automatische Prüfung beim Start ist aktiv.")}
          </p>
        </div>
        <div class="inline-actions">
          {#if update}
            <button
              class="text-button accent"
              disabled={updateBusy}
              onclick={startUpdate}
              type="button"
            >
              Jetzt aktualisieren
            </button>
          {/if}
          <button
            class="text-button"
            disabled={updateBusy}
            onclick={checkUpdate}
            type="button"
          >
            Nach Updates suchen
          </button>
        </div>
      </section>
    </div>
    <footer>TippIT · Sven Labitzki</footer>
  </main>
{/if}

<style>
  :global(body) {
    margin: 0;
    color: #d8dfef;
    background: #151821;
  }
  :global(*) {
    box-sizing: border-box;
  }
  main {
    min-height: 100vh;
    padding: 24px 28px 16px;
    font:
      13px "Segoe UI",
      system-ui,
      sans-serif;
    background: #151821;
  }
  header,
  .section-title,
  .update-section {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  header {
    margin-bottom: 18px;
  }
  h1 {
    margin: 0;
    font-size: 22px;
    font-weight: 650;
  }
  h2 {
    margin: 0 0 10px;
    font-size: 14px;
    font-weight: 650;
    color: #eef2fb;
  }
  h3 {
    margin: 0 0 8px;
    font-size: 12px;
    font-weight: 650;
    color: #aeb8ca;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  p {
    margin: 3px 0 0;
    color: #7f899e;
  }
  .grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 0 28px;
  }
  section {
    min-width: 0;
    padding: 15px 0 12px;
    border-top: 1px solid #2a2f3a;
  }
  .wide {
    grid-column: 1 / -1;
  }
  .line,
  .stack {
    display: flex;
    gap: 14px;
    align-items: center;
    justify-content: space-between;
    min-height: 38px;
    border-top: 1px solid #202530;
  }
  section > .line:first-of-type,
  section > .stack:first-of-type {
    border-top: 0;
  }
  .line span,
  .stack span {
    min-width: 0;
  }
  b {
    display: block;
    font-weight: 500;
  }
  small {
    display: block;
    margin-top: 2px;
    color: #737d91;
  }
  .stack {
    display: block;
    padding: 8px 0;
  }
  .stack > span {
    display: flex;
    justify-content: space-between;
    margin-bottom: 6px;
  }
  output {
    color: #8fa6ca;
  }
  input[type="range"] {
    width: 100%;
    height: 3px;
    accent-color: #82aef0;
  }
  input[type="checkbox"] {
    width: 15px;
    height: 15px;
    accent-color: #78a7ec;
  }
  select,
  input[type="text"] {
    min-width: 132px;
    padding: 5px 7px;
    color: #cbd3e4;
    outline: none;
    background: #1b1f29;
    border: 1px solid #343a48;
    border-radius: 4px;
  }
  select:focus,
  input[type="text"]:focus {
    border-color: #668fc8;
  }
  button {
    font: inherit;
  }
  .value-button {
    padding: 5px 8px;
    color: #a9b8d2;
    cursor: pointer;
    background: transparent;
    border: 1px solid #343a48;
    border-radius: 4px;
  }
  .value-button.recording {
    color: #8eb6f5;
    border-color: #668fc8;
  }
  .text-button {
    padding: 4px 0;
    color: #9ca7ba;
    cursor: pointer;
    background: transparent;
    border: 0;
  }
  .text-button:hover {
    color: #dbe2f0;
  }
  .text-button.accent {
    color: #86b3f4;
  }
  .text-button.danger {
    color: #d9909e;
  }
  .text-button:disabled {
    cursor: default;
    opacity: 0.45;
  }
  .inline-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 14px;
    align-items: center;
  }
  .sync-section {
    padding-top: 18px;
  }
  .section-title {
    margin-bottom: 14px;
  }
  .section-title h2,
  .update-section h2 {
    margin-bottom: 0;
  }
  .sync-state {
    color: #7f899e;
  }
  .sync-state.active {
    color: #83c7a1;
  }
  .sync-state.paused {
    color: #d9b26a;
  }
  .sync-grid {
    display: grid;
    grid-template-columns: 1.15fr 1fr 1.1fr;
    gap: 26px;
  }
  .sync-grid > div {
    min-width: 0;
  }
  .join {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
    margin-top: 9px;
  }
  .join input {
    flex: 1;
    min-width: 150px;
  }
  .pairing {
    display: flex;
    gap: 10px;
    align-items: center;
    margin: 10px 0;
  }
  .qr {
    flex: none;
    width: 72px;
    height: 72px;
    padding: 4px;
    overflow: hidden;
    background: #fff;
  }
  .qr :global(svg) {
    width: 100%;
    height: 100%;
  }
  code {
    font-size: 10px;
    color: #acb8cc;
    word-break: break-all;
    user-select: all;
  }
  details {
    margin-top: 10px;
    color: #737d91;
  }
  summary {
    cursor: pointer;
  }
  .server {
    display: flex;
    gap: 12px;
    align-items: center;
    margin-top: 8px;
  }
  .server input {
    flex: 1;
  }
  .message {
    margin-top: 8px;
    font-size: 12px;
  }
  .error-text,
  .save-state.error {
    color: #e394a4;
  }
  .update-section {
    gap: 20px;
    padding-bottom: 15px;
  }
  .save-state {
    color: #7fc89f;
    opacity: 0;
    transition: opacity 0.15s;
  }
  .save-state.visible {
    opacity: 1;
  }
  footer {
    padding-top: 10px;
    font-size: 11px;
    color: #545d70;
    text-align: right;
  }
  @media (max-width: 820px) {
    .grid {
      grid-template-columns: 1fr;
    }
    .wide {
      grid-column: 1;
    }
    .sync-grid {
      grid-template-columns: 1fr;
    }
  }
</style>
