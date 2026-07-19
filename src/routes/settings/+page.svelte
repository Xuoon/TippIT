<script lang="ts">
  import { onMount } from "svelte";
  import {
    clearHistory,
    getSettings,
    onSettingsChanged,
    type PairingInfo,
    type Settings,
    type SyncStatus,
    setSettings,
    syncCopyCode,
    syncCreateGroup,
    syncJoinGroup,
    syncLeaveGroup,
    syncNewCode,
    syncShowPairing,
    syncStatus,
  } from "$lib/api";

  let settings = $state<Settings | null>(null);
  let tab = $state("allgemein");
  let saveState = $state<"idle" | "saved" | "error">("idle");
  let capturing = $state<"paste" | "history" | null>(null);
  let sync = $state<SyncStatus | null>(null);
  let pairing = $state<PairingInfo | null>(null);
  let joinCode = $state("");
  let syncBusy = $state(false);
  let syncError = $state("");

  const tabs = [
    { id: "allgemein", label: "Allgemein" },
    { id: "hotkeys", label: "Hotkeys" },
    { id: "tippen", label: "Tippen" },
    { id: "historie", label: "Historie" },
    { id: "sync", label: "Sync" },
  ];

  onMount(() => {
    getSettings().then((s) => (settings = s));
    syncStatus().then((s) => (sync = s));
    // Änderungen von außen übernehmen (Tray-Toggle, Remote-Settings-Sync) —
    // sonst pusht der nächste save() hier einen veralteten Snapshot.
    const unlisten = onSettingsChanged((s) => {
      // Echo des eigenen save() ignorieren — sonst springen gerade
      // getippte, noch ungespeicherte Feld-Eingaben (z. B. Server-URL) zurück.
      if (JSON.stringify(s) === JSON.stringify($state.snapshot(settings))) {
        return;
      }
      settings = s;
    });
    return () => {
      unlisten.then((f) => f());
    };
  });

  async function withSync(action: () => Promise<SyncStatus>) {
    syncBusy = true;
    syncError = "";
    try {
      sync = await action();
      pairing = null;
    } catch (e) {
      syncError = String(e);
    }
    syncBusy = false;
  }

  async function togglePairing() {
    if (pairing) {
      pairing = null;
      return;
    }
    syncError = "";
    try {
      pairing = await syncShowPairing();
    } catch (e) {
      syncError = String(e);
    }
  }

  let codeCopied = $state(false);
  async function copyCode() {
    await syncCopyCode();
    codeCopied = true;
    setTimeout(() => (codeCopied = false), 1500);
  }

  async function newCode() {
    const warning = sync?.active
      ? "Es existiert bereits ein TippIT-Code und eine aktive Sync-Gruppe.\n\nEin neuer Code ersetzt den alten und trennt dieses Gerät von der Gruppe. Fortfahren?"
      : "Es existiert bereits ein TippIT-Code.\n\nEin neuer Code ersetzt den alten — bereits gekoppelte Geräte passen dann nicht mehr zusammen. Fortfahren?";
    // biome-ignore lint/suspicious/noAlert: bewusster nativer Bestätigungsdialog
    if (!confirm(warning)) {
      return;
    }
    syncBusy = true;
    syncError = "";
    try {
      pairing = await syncNewCode();
      sync = await syncStatus();
    } catch (e) {
      syncError = String(e);
    }
    syncBusy = false;
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

  const F_KEY_PATTERN = /^F\d{1,2}$/;

  // Layout-unabhängige Taste aus e.code ableiten: e.key liefert bei Shift
  // das Symbol ("!") und ergäbe einen unparsbaren Shortcut — danach wären
  // nach reregister_all ALLE Hotkeys tot.
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

  function captureHotkey(e: KeyboardEvent) {
    if (!(capturing && settings)) {
      return;
    }
    e.preventDefault();
    if (e.key === "Escape") {
      capturing = null;
      return;
    }
    const key = keyFromCode(e.code);
    if (!key) {
      return; // nur Buchstaben, Ziffern und F-Tasten
    }
    const parts: string[] = [];
    if (e.ctrlKey) {
      parts.push("ctrl");
    }
    if (e.shiftKey) {
      parts.push("shift");
    }
    if (e.altKey) {
      parts.push("alt");
    }
    if (e.metaKey) {
      parts.push("super");
    }
    if (parts.length === 0) {
      return; // Modifier verpflichtend
    }
    parts.push(key);
    settings.hotkeys[capturing] = parts.join("+");
    capturing = null;
    save();
  }

  function fmtHotkey(hk: string): string {
    return hk.toUpperCase().replace("CTRL", "STRG").replaceAll("+", " + ");
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
    <aside>
      {#each tabs as t (t.id)}
        <button
          onclick={() => (tab = t.id)}
          type="button"
          class:active={tab === t.id}
        >
          {t.label}
        </button>
      {/each}
      <div class="spacer"></div>
      <span class="status" class:visible={saveState !== "idle"}>
        {saveState === "saved" ? "✓ Gespeichert" : "⚠ Fehler"}
      </span>
    </aside>

    <section>
      {#if tab === "allgemein"}
        <h2>Allgemein</h2>
        <label class="check">
          <input onchange={save} type="checkbox" bind:checked={settings.sounds}>
          Sounds (Beep beim Tippen und Kopieren)
        </label>
        <label class="field">
          Theme
          <select onchange={save} bind:value={settings.theme}>
            <option value="system">System</option>
            <option value="dark">Dunkel</option>
            <option value="light">Hell</option>
          </select>
        </label>
        <p class="hint">Autostart und Pause schaltest du über das Tray-Menü.</p>
      {:else if tab === "hotkeys"}
        <h2>Hotkeys</h2>
        {#each [["paste", "Zwischenablage tippen"], ["history", "Historie öffnen"]] as [ key, label ] (key)}
          <label class="field">
            {label}
            <button
              class="hotkey"
              onclick={() =>
                (capturing =
                  capturing === key ? null : (key as "paste" | "history"))}
              type="button"
              class:capturing={capturing === key}
            >
              {capturing === key
                ? "Tastenkombination drücken… (Esc bricht ab)"
                : fmtHotkey(settings.hotkeys[key as "paste" | "history"])}
            </button>
          </label>
        {/each}
        <p class="hint">
          Ein Modifier (STRG/SHIFT/ALT) ist Pflicht. Schlägt die Registrierung
          fehl (Konflikt mit anderer App), steht ein Hinweis im Log.
        </p>
      {:else if tab === "tippen"}
        <h2>Tippen</h2>
        <label class="field">
          Verzögerung vor dem Tippen: {settings.typing.pre_delay_ms} ms
          <input
            max="3000"
            min="0"
            onchange={save}
            step="100"
            type="range"
            bind:value={settings.typing.pre_delay_ms}
          >
        </label>
        <label class="field">
          Modus
          <select onchange={save} bind:value={settings.typing.mode}>
            <option value="per_char">
              Zeichenweise (Standard, für RDP/Citrix)
            </option>
            <option value="bulk">Alles auf einmal</option>
          </select>
        </label>
        {#if settings.typing.mode === "per_char"}
          <label class="field">
            Verzögerung pro Zeichen: {settings.typing.char_delay_ms} ms
            <input
              max="100"
              min="1"
              onchange={save}
              type="range"
              bind:value={settings.typing.char_delay_ms}
            >
          </label>
        {/if}
        <label class="check">
          <input
            onchange={save}
            type="checkbox"
            bind:checked={settings.typing.trim}
          >
          Leerzeichen am Anfang/Ende entfernen
        </label>
      {:else if tab === "historie"}
        <h2>Historie</h2>
        <label class="field">
          Maximale Einträge: {settings.history.max_entries}
          <input
            max="5000"
            min="100"
            onchange={save}
            step="100"
            type="range"
            bind:value={settings.history.max_entries}
          >
        </label>
        <label class="check">
          <input
            onchange={save}
            type="checkbox"
            bind:checked={settings.history.capture_images}
          >
          Bilder erfassen
        </label>
        <label class="check">
          <input
            onchange={save}
            type="checkbox"
            bind:checked={settings.history.capture_files}
          >
          Kopierte Dateipfade erfassen
        </label>
        <button class="danger" onclick={onClearHistory} type="button">
          Historie löschen (Pins bleiben)
        </button>
      {:else if tab === "sync"}
        <h2>Sync</h2>
        <p class="hint">
          Ende-zu-Ende-verschlüsselter Sync zwischen deinen Geräten — ohne
          Account, Kopplung per TippIT-Code. Der Server sieht nur verschlüsselte
          Daten.
        </p>

        <h3>Dein TippIT-Code</h3>
        <div class="row-buttons">
          <button
            class="primary"
            disabled={syncBusy}
            onclick={copyCode}
            type="button"
          >
            {codeCopied ? "✓ Kopiert" : "Code kopieren"}
          </button>
          <button
            class="primary"
            disabled={syncBusy}
            onclick={togglePairing}
            type="button"
          >
            {pairing ? "Code verbergen" : "Code + QR anzeigen"}
          </button>
          <button
            class="secondary"
            disabled={syncBusy}
            onclick={newCode}
            type="button"
          >
            Neuen Code erstellen
          </button>
        </div>
        {#if pairing}
          <div class="pairing">
            <!-- eslint-disable-next-line svelte/no-at-html-tags -->
            <div class="qr">{@html pairing.qr_svg}</div>
            <code>{pairing.code}</code>
            <p class="hint">
              Diesen Code auf dem zweiten Gerät unter „Mit Code beitreten"
              eingeben. Er enthält den Schlüssel — behandle ihn wie ein Passwort
              und notiere ihn als Wiederherstellungscode.
            </p>
          </div>
        {/if}

        {#if sync?.active}
          <p class="ok">✓ Sync aktiv — Gruppe {sync.group_id}</p>
          <div class="row-buttons">
            <button
              class="danger"
              disabled={syncBusy}
              onclick={() => withSync(syncLeaveGroup)}
              type="button"
            >
              Gruppe verlassen
            </button>
          </div>
          <h3>Was wird synchronisiert?</h3>
          <label class="check">
            <input
              onchange={save}
              type="checkbox"
              bind:checked={settings.sync.sync_text}
            >
            Text-Einträge & Dateipfade
          </label>
          <label class="check">
            <input
              onchange={save}
              type="checkbox"
              bind:checked={settings.sync.sync_settings}
            >
            Einstellungen
          </label>
          <label class="check">
            <input
              onchange={save}
              type="checkbox"
              bind:checked={settings.sync.sync_images}
            >
            Bilder (bis {Math.round(settings.sync.image_max_bytes / 1024)} KB)
          </label>
        {:else}
          <h3>Geräte verbinden</h3>
          <div class="row-buttons">
            <button
              class="primary"
              disabled={syncBusy || !settings.sync.deployment_url}
              onclick={() => withSync(syncCreateGroup)}
              type="button"
            >
              Sync aktivieren (neue Gruppe)
            </button>
          </div>
          <label class="field">
            Mit Code beitreten
            <input
              class="text"
              placeholder="TIPPIT-XXXXX-XXXXX-…"
              spellcheck="false"
              type="text"
              bind:value={joinCode}
            >
          </label>
          <button
            class="primary"
            disabled={syncBusy ||
              joinCode.length < 20 ||
              !settings.sync.deployment_url}
            onclick={() => withSync(() => syncJoinGroup(joinCode))}
            type="button"
          >
            Beitreten
          </button>
        {/if}
        {#if syncError}
          <p class="error">{syncError}</p>
        {/if}
        {#if syncBusy}
          <p class="hint">Verbinde…</p>
        {/if}

        <details class="advanced">
          <summary>Erweitert</summary>
          <label class="field">
            Server-URL
            <input
              class="text"
              onchange={save}
              placeholder="https://….convex.cloud"
              type="text"
              bind:value={settings.sync.deployment_url}
            >
          </label>
        </details>
      {/if}
    </section>
    <span class="brand">TippIT · Sven Labitzki</span>
  </main>
{/if}

<style>
  :global(body) {
    margin: 0;
  }
  main {
    display: flex;
    height: 100vh;
    font-family: "Segoe UI", system-ui, sans-serif;
    font-size: 13px;
    color: #cdd6f4;
    background: #16161f;
  }
  aside {
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    gap: 4px;
    width: 150px;
    padding: 14px 10px;
    border-right: 1px solid #27273a;
  }
  aside button {
    padding: 8px 12px;
    font-size: 13px;
    color: #a6adc8;
    text-align: left;
    cursor: pointer;
    background: transparent;
    border: none;
    border-radius: 8px;
  }
  aside button.active {
    font-weight: 600;
    color: #cdd6f4;
    background: #27273a;
  }
  .spacer {
    flex: 1;
  }
  .status {
    padding: 0 12px;
    font-size: 12px;
    color: #a6e3a1;
    opacity: 0;
    transition: opacity 0.2s;
  }
  .status.visible {
    opacity: 1;
  }
  section {
    flex: 1;
    padding: 18px 26px;
    overflow-y: auto;
  }
  h2 {
    margin: 0 0 16px;
    font-size: 17px;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
    max-width: 380px;
    margin-bottom: 16px;
    color: #a6adc8;
  }
  .check {
    display: flex;
    gap: 8px;
    align-items: center;
    margin-bottom: 12px;
  }
  select,
  .hotkey {
    padding: 7px 10px;
    font-size: 13px;
    color: #cdd6f4;
    background: #1e1e2e;
    border: 1px solid #313244;
    border-radius: 8px;
  }
  .hotkey {
    text-align: left;
    cursor: pointer;
  }
  .hotkey.capturing {
    color: #89b4fa;
    border-color: #89b4fa;
  }
  .hint {
    max-width: 420px;
    font-size: 12px;
    color: #7f849c;
  }
  .danger {
    padding: 8px 14px;
    margin-top: 8px;
    color: #f38ba8;
    cursor: pointer;
    background: #302030;
    border: 1px solid #45304a;
    border-radius: 8px;
  }
  .danger:hover {
    background: #3c2840;
  }
  .text {
    padding: 7px 10px;
    font-size: 13px;
    color: #cdd6f4;
    background: #1e1e2e;
    border: 1px solid #313244;
    border-radius: 8px;
  }
  .primary {
    padding: 8px 14px;
    font-weight: 600;
    color: #11111b;
    cursor: pointer;
    background: #89b4fa;
    border: none;
    border-radius: 8px;
  }
  .primary:disabled {
    cursor: default;
    opacity: 0.5;
  }
  .row-buttons {
    display: flex;
    gap: 8px;
    margin: 8px 0 16px;
  }
  .ok {
    color: #a6e3a1;
  }
  .error {
    color: #f38ba8;
  }
  .pairing {
    max-width: 420px;
    padding: 14px;
    margin-bottom: 16px;
    background: #1e1e2e;
    border: 1px solid #313244;
    border-radius: 10px;
  }
  .pairing .qr {
    width: fit-content;
    padding: 8px;
    margin-bottom: 10px;
    background: white;
    border-radius: 8px;
  }
  .pairing code {
    display: block;
    font-size: 12px;
    color: #f9e2af;
    word-break: break-all;
    user-select: all;
  }
  h3 {
    margin: 18px 0 10px;
    font-size: 14px;
  }
  .secondary {
    padding: 8px 14px;
    color: #a6adc8;
    cursor: pointer;
    background: #1e1e2e;
    border: 1px solid #313244;
    border-radius: 8px;
  }
  .secondary:hover {
    color: #cdd6f4;
  }
  .secondary:disabled {
    cursor: default;
    opacity: 0.5;
  }
  .advanced {
    margin-top: 22px;
    color: #7f849c;
  }
  .advanced summary {
    margin-bottom: 8px;
    font-size: 12px;
    cursor: pointer;
  }
  .brand {
    position: fixed;
    right: 14px;
    bottom: 10px;
    font-size: 11px;
    font-weight: 600;
    color: #585b70;
    -webkit-text-fill-color: transparent;
    pointer-events: none;
    background: linear-gradient(90deg, #89b4fa, #cba6f7);
    -webkit-background-clip: text;
    background-clip: text;
  }
</style>
