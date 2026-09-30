<script lang="ts">
  import { getVersion } from "@tauri-apps/api/app";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount } from "svelte";
  import {
    checkForUpdate,
    clearHistory,
    exportHistory,
    forgetHistoryPosition,
    getDefaultSettings,
    getSettings,
    type ImportReport,
    importHistory,
    installUpdate,
    listMonitors,
    type MonitorInfo,
    onSettingsChanged,
    onSettingsTab,
    onUpdateProgress,
    openDataDir,
    type PaletteModifier,
    pendingUpdate,
    restartApp,
    type Settings,
    setSettings,
    settingsWindowReady,
    takeSettingsTab,
    type UpdateMetadata,
    type UpdateProgress,
  } from "$lib/api";
  import {
    groupTone,
    parseChangelog,
    releaseDate,
    releaseName,
  } from "$lib/changelog";
  import Menu, { type MenuItem } from "$lib/components/menu.svelte";
  import Modal from "$lib/components/modal.svelte";
  import Select from "$lib/components/select.svelte";
  import {
    placementKey,
    REMEMBERED,
    screenFromKey,
    screenOptions,
  } from "$lib/components/settings/history-screen";
  import { paletteCounts } from "$lib/components/settings/palette";
  import PermissionsPane from "$lib/components/settings/permissions-pane.svelte";
  import StepperRow from "$lib/components/stepper-row.svelte";
  import SwitchRow from "$lib/components/switch-row.svelte";
  import { hotkeyFromEvent, sameHotkey } from "$lib/hotkey-capture";
  import Icon from "$lib/icon.svelte";
  import {
    formatHotkey,
    hasPermissionsTab,
    paletteModifierChoices,
    paletteModifierLabel,
  } from "$lib/platform";
  import { SHORTCUTS } from "$lib/shortcuts";
  import { initTheme, setThemeMode } from "$lib/theme";
  import changelogRaw from "../../../CHANGELOG.md?raw";
  import "$lib/theme.css";
  import "$lib/components/settings/settings.css";

  let settings = $state<Settings | null>(null);
  let defaults = $state<Settings | null>(null);
  let saveState = $state<"idle" | "saved" | "error">("idle");
  let capturing = $state<"history" | "paste" | null>(null);
  /** Der gerade gedrückte Hotkey gehört schon der anderen Aktion. */
  let hotkeyTaken = $state(false);
  let appVersion = $state("");
  let changelogOpen = $state(false);
  let helpOpen = $state(false);
  let confirmClearOpen = $state(false);
  let backupOpen = $state(false);
  let permissionsOpen = $state(false);
  let paletteMenuOpen = $state(false);
  let monitors = $state<MonitorInfo[]>([]);

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
  let updateDone = $state(false);
  /** Fortschritt in Prozent; null = unbestimmt (Server ohne Content-Length). */
  const updatePercent = $derived.by(() => {
    const total = updateProgress?.total ?? 0;
    if (!(updateProgress && total > 0)) {
      return null;
    }
    return Math.min(100, Math.round((updateProgress.downloaded / total) * 100));
  });

  /** Aufnehmbare Hotkeys: Klick auf die Keycaps startet direkt die Aufnahme. */
  const HERO_HOTKEYS = [
    { key: "paste", label: "Tippen" },
    { key: "history", label: "Historie" },
  ] as const;

  const overlayOpen = () =>
    changelogOpen ||
    helpOpen ||
    confirmClearOpen ||
    backupOpen ||
    permissionsOpen;

  function closeOverlays() {
    changelogOpen = false;
    helpOpen = false;
    confirmClearOpen = false;
    backupOpen = false;
    permissionsOpen = false;
    paletteMenuOpen = false;
  }

  /** Backend-Vertrag: „berechtigungen" öffnet die Berechtigungen. */
  function openTab(id: string) {
    if (id !== "berechtigungen" || !hasPermissionsTab) {
      return;
    }
    closeOverlays();
    capturing = null;
    permissionsOpen = true;
  }

  // CHANGELOG.md wird per Vite ?raw in die App gebündelt.
  const CHANGELOG = parseChangelog(changelogRaw);

  function loadMonitors() {
    listMonitors()
      .then((list) => (monitors = list))
      .catch(() => {
        // Ohne Liste bleiben Mauszeiger und Hauptmonitor wählbar.
      });
  }

  const screenChoices = $derived(
    settings
      ? screenOptions(
          monitors,
          settings.history.window_screen,
          settings.history.window_position
        )
      : []
  );

  /** Eine Regel zu wählen verwirft die verschobene Position, auch wenn es
      dieselbe Regel ist. */
  async function setScreen(key: string) {
    if (!settings || key === REMEMBERED) {
      return;
    }
    settings.history.window_screen = screenFromKey(key);
    settings.history.window_position = null;
    await save();
    await forgetPosition();
  }

  async function forgetPosition() {
    if (!settings) {
      return;
    }
    settings.history.window_position = null;
    try {
      await forgetHistoryPosition();
    } catch {
      saveState = "error";
    }
  }

  onMount(() => {
    const stopTheme = initTheme();
    getVersion()
      .then((v) => (appVersion = v))
      .catch(() => {
        // Ohne Version bleibt die Fußzeile beim Namen.
      });
    // Nur die Einstellungen sind Pflicht; ohne Update-Info oder Defaults
    // bleibt das Fenster trotzdem bedienbar.
    Promise.all([
      getSettings(),
      pendingUpdate().catch(() => null),
      getDefaultSettings().catch(() => null),
    ])
      .then(async ([loadedSettings, loadedUpdate, loadedDefaults]) => {
        settings = loadedSettings;
        update = loadedUpdate;
        defaults = loadedDefaults;
        await settingsWindowReady();
      })
      .catch(async () => {
        await settingsWindowReady();
      });

    takeSettingsTab()
      .then((tab) => {
        if (tab) {
          openTab(tab);
        }
      })
      .catch(() => undefined);
    const unlistenTab = onSettingsTab(openTab);

    // Monitore kommen und gehen, während das Fenster offen ist.
    loadMonitors();
    window.addEventListener("focus", loadMonitors);

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
        // Rust hat das Update bereits verbraucht: ein erneuter Klick auf
        // „Update auf …" liefe ins Leere, deshalb nur noch Neustart anbieten.
        updateMessage = "Installiert — TippIT neu starten.";
        update = null;
        updateDone = true;
        updateBusy = false;
      }
    });

    return () => {
      stopTheme();
      window.removeEventListener("focus", loadMonitors);
      unlisten.then((stop) => stop());
      unlistenTab.then((stop) => stop());
      unlistenUpdate.then((stop) => stop());
    };
  });

  /** Doppelklick auf einen Wert: zurück auf den Auslieferungs-Default. */
  function resetTo<S extends "history" | "typing">(
    group: S,
    field: keyof Settings[S]
  ) {
    if (settings && defaults) {
      settings[group][field] = defaults[group][field];
      save();
    }
  }

  const THEMES = [
    { label: "System", value: "system" },
    { label: "Dunkel", value: "dark" },
    { label: "Hell", value: "light" },
  ] as const;

  function setTheme(value: string) {
    if (!settings) {
      return;
    }
    settings.theme = value;
    // Sofort anwenden, nicht erst nach dem Save-Roundtrip.
    setThemeMode(value);
    save();
  }

  function setPerChar(on: boolean) {
    if (settings) {
      settings.typing.mode = on ? "per_char" : "bulk";
    }
  }

  const RETENTIONS = [
    { label: "Nie", value: 0 },
    { label: "Nach 7 Tagen", value: 7 },
    { label: "Nach 30 Tagen", value: 30 },
    { label: "Nach 90 Tagen", value: 90 },
    { label: "Nach 1 Jahr", value: 365 },
  ] as const;

  /** Palette-Menü: an/aus, Modifier zum Öffnen und zum Tippen. */
  const paletteMenu = $derived.by((): MenuItem[][] => {
    if (!settings) {
      return [];
    }
    const palette = settings.palette;
    const pick =
      (field: "modifier" | "type_modifier", value: PaletteModifier) => () => {
        palette[field] = value;
        paletteMenuOpen = false;
        save();
      };
    return [
      [
        {
          checked: palette.enabled,
          label: "Palette aktiv",
          onselect: () => {
            palette.enabled = !palette.enabled;
            save();
          },
          role: "check",
        },
      ],
      paletteModifierChoices(palette.type_modifier).map((c) => ({
        checked: palette.modifier === c.value,
        disabled: c.disabled,
        label: `Öffnen mit ${paletteModifierLabel(c.value)}`,
        onselect: pick("modifier", c.value),
      })),
      paletteModifierChoices(palette.modifier).map((c) => ({
        checked: palette.type_modifier === c.value,
        disabled: c.disabled,
        label: `Tippen mit ${paletteModifierLabel(c.value)}`,
        onselect: pick("type_modifier", c.value),
      })),
      paletteCounts(palette.count).map((n) => ({
        checked: palette.count === n,
        label: n === 1 ? "1 Eintrag" : `${n} Einträge`,
        onselect: () => {
          palette.count = n;
          paletteMenuOpen = false;
          save();
        },
      })),
    ];
  });

  function togglePaletteMenu() {
    capturing = null;
    paletteMenuOpen = !paletteMenuOpen;
  }

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
          (report.skipped > 0 ? `, ${report.skipped} bereits vorhanden.` : ".") +
          (report.over_limit > 0
            ? ` ${report.over_limit} Einträge liegen über dem Limit, die nächste Kopie entfernt die ältesten ungepinnten endgültig.`
            : "");
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

  function onUpdateClick() {
    if (updateDone) {
      restartApp().catch(() => {
        // Rust-Log
      });
    } else if (update) {
      startUpdate();
    } else {
      checkUpdate();
    }
  }

  /** Beschriftung des Update-Knopfs; zeigt während des Ladens den Fortschritt. */
  const updateLabel = $derived.by(() => {
    if (updateDone) {
      return "Jetzt neu starten";
    }
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

  function toggleCapture(key: "history" | "paste") {
    paletteMenuOpen = false;
    capturing = capturing === key ? null : key;
    hotkeyTaken = false;
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key === "Escape" && paletteMenuOpen) {
      paletteMenuOpen = false;
      return;
    }
    if (event.key === "Escape" && overlayOpen()) {
      closeOverlays();
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
    const hotkey = hotkeyFromEvent(event);
    if (!hotkey) {
      return;
    }
    // Zweimal dasselbe Kürzel ließe sich nur einmal registrieren.
    const other = capturing === "paste" ? "history" : "paste";
    if (sameHotkey(hotkey, settings.hotkeys[other])) {
      hotkeyTaken = true;
      return;
    }
    settings.hotkeys[capturing] = hotkey;
    capturing = null;
    save();
  }

  async function clearConfirmed() {
    confirmClearOpen = false;
    await clearHistory().catch(() => {
      // Rust-Log
    });
  }
</script>

<svelte:window
  onkeydown={onKeydown}
  onpointerdown={() => (paletteMenuOpen = false)}
/>

{#snippet caps(list: string[])}
  {#each list as cap, i (i)}
    {#if i > 0}
      <span class="plus">+</span>
    {/if}
    <kbd class="cap">{cap}</kbd>
  {/each}
{/snippet}

{#if settings}
  <main>
    <!-- Hotkeys: Klick auf die Keycaps nimmt neu auf, die Palette öffnet ein Menü. -->
    <header class="hero">
      {#each HERO_HOTKEYS as hk (hk.key)}
        <button
          aria-pressed={capturing === hk.key}
          class="hk"
          onclick={() => toggleCapture(hk.key)}
          title="Klicken und neue Tasten drücken (Esc bricht ab)"
          type="button"
          class:recording={capturing === hk.key}
        >
          <span class="caps">
            {#if capturing === hk.key}
              <span class="rec" class:taken={hotkeyTaken}>
                {hotkeyTaken ? "Schon vergeben…" : "Tasten drücken…"}
              </span>
            {:else}
              {@render caps(formatHotkey(settings.hotkeys[hk.key]).split(" + "))}
            {/if}
          </span>
          <span class="hk-label">{hk.label}</span>
        </button>
      {/each}
      <!-- pointerdown bleibt hier: der Fenster-Handler schlösse das Menü
           sonst vor dem click, der es wieder öffnete. -->
      <div
        class="hk-wrap"
        onpointerdown={(e) => e.stopPropagation()}
        role="presentation"
      >
        <button
          aria-expanded={paletteMenuOpen}
          aria-haspopup="menu"
          class="hk"
          onclick={togglePaletteMenu}
          title="Modifier wählen"
          type="button"
          class:off={!settings.palette.enabled}
          class:recording={paletteMenuOpen}
        >
          <span class="caps">
            {@render caps([
              paletteModifierLabel(settings.palette.modifier),
              "Klick",
            ])}
          </span>
          <span class="hk-label">
            {settings.palette.enabled ? "Palette" : "Palette aus"}
          </span>
        </button>
        {#if paletteMenuOpen}
          <Menu align="left" label="Palette" sections={paletteMenu} />
        {/if}
      </div>
      <!-- Fest ESC (kein Setting): nur während eines Tipp-Vorgangs registriert. -->
      <div class="hk static">
        <span class="caps"><kbd class="cap">Esc</kbd></span>
        <span class="hk-label">Abbrechen</span>
      </div>
    </header>

    <div class="grid">
      <section class="tile">
        <h2>Tippen</h2>
        <SwitchRow
          label="Zeichenweise"
          onchange={save}
          bind:checked={
            () => settings?.typing.mode === "per_char",
            setPerChar
          }
        />
        <StepperRow
          disabled={settings.typing.mode !== "per_char"}
          label="Zeichenabstand"
          max={100}
          min={1}
          onchange={save}
          onreset={() => resetTo("typing", "char_delay_ms")}
          step={5}
          unit="ms"
          bind:value={settings.typing.char_delay_ms}
        />
        <StepperRow
          label="Startverzögerung"
          max={3000}
          min={0}
          onchange={save}
          onreset={() => resetTo("typing", "pre_delay_ms")}
          step={100}
          unit="ms"
          bind:value={settings.typing.pre_delay_ms}
        />
        <SwitchRow
          label="Leerraum entfernen"
          onchange={save}
          bind:checked={settings.typing.trim}
        />
      </section>

      <section class="tile">
        <h2>Historie-Fenster</h2>
        <div class="row">
          <label class="row-label" for="window-screen">Öffnen auf</label>
          <span class="row-actions">
            {#if settings.history.window_position}
              <button
                aria-label="Position zurücksetzen"
                class="iconbtn"
                onclick={forgetPosition}
                title="Verschobene Position vergessen"
                type="button"
              >
                <Icon name="restore" size={14} />
              </button>
            {/if}
            <Select
              id="window-screen"
              onchange={setScreen}
              options={screenChoices}
              value={placementKey(
                settings.history.window_screen,
                settings.history.window_position
              )}
            />
          </span>
        </div>
        <StepperRow
          label="Größe"
          max={90}
          min={40}
          onchange={save}
          onreset={() => resetTo("history", "window_size")}
          step={2}
          unit="%"
          bind:value={settings.history.window_size}
        />
        <SwitchRow
          label="Klick außerhalb schließt"
          onchange={save}
          bind:checked={settings.history.close_on_blur}
        />
        <div class="row">
          <label class="row-label" for="theme">Darstellung</label>
          <Select
            id="theme"
            onchange={setTheme}
            options={THEMES}
            value={settings.theme}
          />
        </div>
      </section>

      <section class="tile">
        <h2>Erfassen</h2>
        <SwitchRow
          label="Bilder"
          onchange={save}
          bind:checked={settings.history.capture_images}
        />
        <SwitchRow
          label="Dateipfade"
          onchange={save}
          bind:checked={settings.history.capture_files}
        />
        <SwitchRow
          label="Formatierung"
          onchange={save}
          bind:checked={settings.history.capture_html}
        />
        <div class="row">
          <label class="row-label" for="exclude">Ausgenommen</label>
          <div class="tags">
            {#each settings.history.excluded_apps as app (app)}
              <button
                aria-label="{app} entfernen"
                class="tag"
                onclick={() => removeExcluded(app)}
                title="Entfernen"
                type="button"
              >
                {app}<Icon name="x" size={10} />
              </button>
            {/each}
            <input
              class="tag-input"
              id="exclude"
              onblur={addExcluded}
              onkeydown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  addExcluded();
                }
              }}
              placeholder={settings.history.excluded_apps.length > 0
                ? "+"
                : "Programm, Enter"}
              spellcheck="false"
              type="text"
              bind:value={excludeInput}
            >
          </div>
        </div>
      </section>

      <section class="tile">
        <h2>Aufbewahrung</h2>
        <StepperRow
          label="Maximal"
          max={5000}
          min={100}
          onchange={save}
          onreset={() => resetTo("history", "max_entries")}
          step={100}
          bind:value={settings.history.max_entries}
        />
        <div class="row">
          <label class="row-label" for="retention">Aufräumen</label>
          <Select
            id="retention"
            onchange={save}
            options={RETENTIONS}
            bind:value={settings.history.retention_days}
          />
        </div>
        <SwitchRow label="Töne" onchange={save} bind:checked={settings.sounds} />
        <div class="row">
          <span class="row-label">Historie leeren</span>
          <button
            class="btn danger"
            onclick={() => (confirmClearOpen = true)}
            type="button"
          >
            <Icon name="trash" size={14} />Leeren…
          </button>
        </div>
      </section>
    </div>

    <!-- Fußzeile: Version, Seltenes als Overlay, Update und Hilfe. -->
    <footer>
      <span class="ver">TippIT {appVersion ? `v${appVersion}` : ""}</span>
      <button class="link" onclick={() => (changelogOpen = true)} type="button">
        Was ist neu?
      </button>
      <button class="link" onclick={() => (backupOpen = true)} type="button">
        Sicherung
      </button>
      {#if hasPermissionsTab}
        <button
          class="link"
          onclick={() => openTab("berechtigungen")}
          type="button"
        >
          Berechtigungen
        </button>
      {/if}
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
        onclick={onUpdateClick}
        title={updateMessage || undefined}
        type="button"
        style:--p="{updateBusy ? (updatePercent ?? 0) : 0}%"
        class:loading={updateBusy}
        class:primary={Boolean(update) || updateDone}
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

  {#if backupOpen}
    <Modal onclose={() => (backupOpen = false)} title="Sicherung">
      <div class="card">
        <div class="row stack">
          <span class="row-label">
            Sicherungsdatei
            <span class="row-hint">Für den Umzug auf einen anderen Rechner, verschlüsselt mit Passwort</span>
          </span>
          <div class="inline">
            <input
              aria-label="Passwort der Sicherung"
              autocomplete="new-password"
              class="input"
              placeholder="Passwort, mind. 8 Zeichen"
              type="password"
              bind:value={backupPassword}
            >
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
            <span class="row-hint">Schlüssel wird abgeleitet…</span>
          {:else if backupMessage}
            <span class="ok-row"><Icon name="check" size={13} />{backupMessage}</span>
          {:else if backupError}
            <span class="err-line"><Icon name="alert" size={13} />{backupError}</span>
          {/if}
        </div>
        <div class="row">
          <span class="row-label">
            Datenordner
            <span class="row-hint mono">~/.labi/tippit</span>
          </span>
          <button class="btn" onclick={openFolder} type="button">
            <Icon name="external" size={14} />Öffnen
          </button>
        </div>
      </div>
    </Modal>
  {/if}

  {#if permissionsOpen}
    <Modal onclose={() => (permissionsOpen = false)} title="Berechtigungen">
      <PermissionsPane />
    </Modal>
  {/if}

  <!-- Hilfe-Overlay: bewusst kein fester Bereich mehr — nur bei Bedarf. -->
  {#if helpOpen}
    <Modal onclose={() => (helpOpen = false)} title="Hilfe">
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
          {#each SHORTCUTS.filter((s) => s.settings) as shortcut (shortcut.keys)}
            <div class="hlp-row">
              <span>{shortcut.label}</span>
              <kbd class="fixed-key">{shortcut.keys}</kbd>
            </div>
          {/each}
        </section>
      </div>
      <div class="hlp-foot">
        <p class="hlp-text">
          {#if hasPermissionsTab}
            Nichts passiert? Dann fehlt meist die Freigabe für die
            Bedienungshilfen:
            <button
              class="pathlink"
              onclick={() => openTab("berechtigungen")}
              type="button"
            >
              Berechtigungen prüfen
            </button>
          {:else}
            Nichts passiert? In Fenstern mit Administratorrechten kann TippIT
            nur tippen, wenn es selbst mit Administratorrechten läuft.
          {/if}
        </p>
        <p class="hlp-text">
          Logs:
          <button class="pathlink" onclick={openFolder} type="button">
            <code>~/.labi/tippit/</code>
          </button>
        </p>
      </div>
    </Modal>
  {/if}

  {#if changelogOpen}
    <Modal onclose={() => (changelogOpen = false)} title="Was ist neu?">
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
            <div class="log-group tone-{groupTone(group.title)}">
              <span class="log-dot"></span>{group.title}
            </div>
            <ul class="tone-{groupTone(group.title)}">
              {#each group.items as item (item)}
                <li>{item}</li>
              {/each}
            </ul>
          {/each}
        </details>
      {/each}
    </Modal>
  {/if}

  <!-- Eigener Dialog statt window.confirm: WKWebView zeigt confirm() unter
       Tauri nicht an und liefert still false (wry implementiert das Panel nicht). -->
  {#if confirmClearOpen}
    <Modal
      compact
      onclose={() => (confirmClearOpen = false)}
      title="Historie leeren?"
    >
      <p class="confirm-text">
        Alle Einträge außer Angepinntem und Textbausteinen wandern in den
        Papierkorb. Im Historie-Fenster lassen sie sich 30 Tage lang
        wiederherstellen.
      </p>
      <div class="confirm-actions">
        <button
          class="btn"
          onclick={() => (confirmClearOpen = false)}
          type="button"
        >
          Abbrechen
        </button>
        <button class="btn danger solid" onclick={clearConfirmed} type="button">
          <Icon name="trash" size={14} />In den Papierkorb
        </button>
      </div>
    </Modal>
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

  /* ---- Hero: Hotkeys als Keycaps ---- */
  .hero {
    display: flex;
    flex: none;
    gap: 8px;
    align-items: stretch;
    padding: 8px 14px;
    background: var(--bg-sunken);
    border-bottom: 1px solid var(--border);
  }
  .hk {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: 6px;
    align-items: center;
    justify-content: center;
    min-width: 0;
    padding: 6px 8px;
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
  .hk-wrap {
    position: relative;
    display: flex;
    flex: 1;
    min-width: 0;
  }
  .hk.static {
    flex: 0.55;
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
    height: 28px;
  }
  .hk.off .caps {
    opacity: 0.45;
  }
  .cap {
    display: grid;
    place-items: center;
    min-width: 28px;
    height: 28px;
    padding: 0 7px;
    font: 600 var(--fs-control) / 1 var(--font-ui);
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
  .rec.taken {
    color: var(--warn);
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
    font: 450 var(--fs-micro) / 1 var(--font-ui);
    color: var(--fg-muted);
    white-space: nowrap;
  }
  /* ---- Raster: zwei Spalten Kacheln, passt ohne Scrollen in 720 × 560 ---- */
  .grid {
    display: grid;
    flex: 1;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 12px;
    align-content: start;
    min-height: 0;
    padding: 14px 18px;
    overflow-y: auto;
  }
  .tile h2 {
    padding: 10px 0 6px;
    margin: 0;
    font: 600 var(--fs-label) / 1.2 var(--font-ui);
    color: var(--fg);
  }

  /* Ausgenommene Programme: Tags mit Eingabe in derselben Zeile. */
  .tags {
    display: flex;
    flex: 1;
    flex-wrap: wrap;
    gap: 4px;
    align-items: center;
    justify-content: flex-end;
    min-width: 0;
    min-height: 28px;
    max-height: 60px;
    padding: 2px;
    overflow-y: auto;
    background: var(--bg-base);
    border-radius: var(--r-md);
  }
  .tags:focus-within {
    box-shadow: inset 0 0 0 1px var(--border-focus);
  }
  .tag {
    display: inline-flex;
    gap: 6px;
    align-items: center;
    height: 22px;
    padding: 0 7px 0 9px;
    font: 500 var(--fs-button) / 1 var(--font-ui);
    color: var(--fg-body);
    cursor: pointer;
    background: var(--bg-strong);
    border: 0;
    border-radius: var(--r-full);
  }
  .tag :global(.ic) {
    color: var(--fg-dim);
  }
  .tag:hover :global(.ic) {
    color: var(--danger);
  }
  .tag:focus-visible {
    outline: none;
    box-shadow: var(--shadow-focus);
  }
  .tag-input {
    flex: 1 1 24px;
    min-width: 24px;
    height: 22px;
    padding: 0 6px;
    font: 400 var(--fs-control) / 1 var(--font-ui);
    color: var(--fg);
    outline: none;
    background: transparent;
    border: 0;
  }
  .tag-input:focus {
    min-width: 96px;
  }
  .tag-input::placeholder {
    color: var(--fg-placeholder);
  }

  .row-actions {
    display: flex;
    flex: none;
    gap: 6px;
    align-items: center;
  }
  .mono {
    font-family: var(--font-mono);
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
  .link {
    padding: 0;
    font-size: var(--fs-micro);
    color: var(--accent-text);
    white-space: nowrap;
    cursor: pointer;
    background: transparent;
    border: 0;
  }
  .link:hover {
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

  /* ---- Hilfe: zwei Spalten, solange die Breite reicht ---- */
  .fixed-key {
    padding: 3px 8px;
    font: 500 var(--fs-micro) / 1 var(--font-ui);
    color: var(--fg-muted);
    white-space: nowrap;
    background: var(--bg-strong);
    border-radius: var(--r-sm);
  }
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
  .hlp-foot {
    padding-top: 20px;
    margin-top: auto;
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

  /* ---- Changelog: pro Release aufklappbar, neuester offen ---- */
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

  /* ---- Bestätigung „Historie leeren" ---- */
  .confirm-text {
    margin: 0 0 16px;
    font-size: var(--fs-control);
    line-height: 1.5;
    color: var(--fg-body);
  }
  .confirm-actions {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
  }

  /* Nach den allgemeinen `code`-Regeln (.hlp-text), sonst überdecken sie die
     Akzentfarbe des Log-Pfads. */
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
  .pathlink:hover,
  .pathlink:hover code {
    text-decoration: underline;
  }
</style>
