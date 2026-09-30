<script lang="ts">
  import { getVersion } from "@tauri-apps/api/app";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount, tick } from "svelte";
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
  import Modal from "$lib/components/modal.svelte";
  import Select from "$lib/components/select.svelte";
  import {
    placementKey,
    REMEMBERED,
    screenFromKey,
    screenOptions,
  } from "$lib/components/settings/history-screen";
  import PermissionsPane from "$lib/components/settings/permissions-pane.svelte";
  import SliderRow from "$lib/components/slider-row.svelte";
  import SwitchRow from "$lib/components/switch-row.svelte";
  import { hotkeyFromEvent, sameHotkey } from "$lib/hotkey-capture";
  import Icon from "$lib/icon.svelte";
  import { formatHotkey, hasPermissionsTab } from "$lib/platform";
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
  let navQuery = $state("");

  /** Hero-Hotkeys: Klick auf die Keycaps startet direkt die Aufnahme. */
  const HERO_HOTKEYS = [
    { key: "paste", kw: "hkPaste", label: "Zwischenablage tippen" },
    { key: "history", kw: "hkHistory", label: "Historie öffnen" },
  ] as const;

  /** Eine Seite, gruppiert: die Seitenleiste springt nur zu den Abschnitten. */
  const SECTIONS = [
    {
      icon: "sliders",
      id: "allgemein",
      keys: ["theme", "sounds"],
      label: "Allgemein",
    },
    {
      icon: "cursor-text",
      id: "tippen",
      keys: ["typeMode", "charDelay", "preDelay", "trim"],
      label: "Tippen",
    },
    {
      icon: "app",
      id: "fenster",
      keys: ["winScreen", "winSize", "closeOnBlur"],
      label: "Historie-Fenster",
    },
    {
      icon: "clipboard",
      id: "erfassen",
      keys: ["capImages", "capFiles", "capHtml", "excludeApps"],
      label: "Erfassen",
    },
    {
      icon: "clock",
      id: "aufbewahrung",
      keys: ["maxEntries", "retention", "clearHistory"],
      label: "Aufbewahrung",
    },
    {
      icon: "save",
      id: "daten",
      keys: ["backup", "dataDir"],
      label: "Sicherung",
    },
    {
      icon: "shield",
      id: "berechtigungen",
      keys: ["permAx", "permLocation", "permDiag"],
      label: "Berechtigungen",
    },
  ] as const;
  type SectionId = (typeof SECTIONS)[number]["id"];
  const VISIBLE_SECTIONS = SECTIONS.filter(
    (s) => s.id !== "berechtigungen" || hasPermissionsTab
  );
  const section = (id: SectionId) =>
    VISIBLE_SECTIONS.find((s) => s.id === id);
  /** Frühere Tab-IDs aus dem Backend-Vertrag auf Abschnitte abbilden. */
  const LEGACY_TABS: Record<string, SectionId> = { historie: "fenster" };
  let activeSection = $state<SectionId>("allgemein");
  let pane = $state<HTMLDivElement>();

  function scrollToSection(id: SectionId, behavior: ScrollBehavior = "smooth") {
    activeSection = id;
    const el = document.getElementById(`sec-${id}`);
    if (!(el && pane)) {
      return;
    }
    // Direkt am Scrollbereich statt scrollIntoView: das scrollt auch die
    // Vorfahren mit und bricht ab, sobald die Seite neu rendert.
    const top =
      el.getBoundingClientRect().top -
      pane.getBoundingClientRect().top +
      pane.scrollTop;
    pane.scrollTo({ behavior, top: Math.max(0, top - 4) });
  }

  /** Abschnitt aus dem Backend (z. B. fehlende Berechtigung) nach vorn holen. */
  function openTab(id: string) {
    const target = section((LEGACY_TABS[id] ?? id) as SectionId);
    if (!target) {
      return;
    }
    navQuery = "";
    helpOpen = false;
    changelogOpen = false;
    confirmClearOpen = false;
    pendingSection = target.id;
  }

  // Springen erst, wenn die Seite gerendert ist: beim Öffnen kommt der
  // Abschnitt oft vor den Einstellungen an.
  let pendingSection = $state<SectionId | null>(null);
  $effect(() => {
    const id = pendingSection;
    if (!(id && settings && pane)) {
      return;
    }
    tick().then(() => {
      scrollToSection(id, "instant");
      pendingSection = null;
    });
  });

  /** Scroll-Spy: der oberste angeschnittene Abschnitt ist aktiv. */
  function onPaneScroll() {
    if (!pane || q !== "") {
      return;
    }
    const top = pane.getBoundingClientRect().top + 24;
    let current: SectionId = VISIBLE_SECTIONS[0].id;
    for (const s of VISIBLE_SECTIONS) {
      const el = document.getElementById(`sec-${s.id}`);
      if (el && el.getBoundingClientRect().top <= top) {
        current = s.id;
      }
    }
    // Ganz unten gewinnt der letzte Abschnitt, auch wenn er kurz ist.
    if (pane.scrollTop + pane.clientHeight >= pane.scrollHeight - 2) {
      current = VISIBLE_SECTIONS.at(-1)?.id ?? current;
    }
    activeSection = current;
  }

  // CHANGELOG.md wird per Vite ?raw in die App gebündelt.
  const CHANGELOG = parseChangelog(changelogRaw);

  const KEYWORDS: Record<string, string> = {
    sounds: "sounds ton akustik signal beep piepsen lautstärke",
    theme: "darstellung theme design aussehen hell dunkel dark light system",
    hkPaste:
      "hotkey tastenkürzel zwischenablage tippen einfügen strg cmd e shortcut",
    hkHistory:
      "hotkey tastenkürzel historie öffnen verlauf strg cmd shift e shortcut",
    hkEsc: "hotkey abbrechen stopp escape esc tippen anhalten",
    preDelay: "startverzögerung verzögerung delay wartezeit vorlauf tippen",
    typeMode:
      "modus zeichenweise auf einmal bulk per char tippen geschwindigkeit",
    charDelay:
      "zeichenabstand tempo geschwindigkeit delay tippen millisekunden",
    trim: "leerraum entfernen trim whitespace leerzeichen kürzen",
    maxEntries: "maximale einträge anzahl limit historie größe aufbewahren",
    winSize:
      "fenstergröße fenster größe skalierung prozent historie breite höhe zoom bildschirm",
    winScreen:
      "monitor bildschirm display anzeige position öffnen hauptmonitor mauszeiger maus zweiter bildschirm fenster verschieben ziehen zuletzt zurücksetzen",
    closeOnBlur:
      "schließen klick außerhalb daneben fokus verlieren ausblenden automatisch fenster historie",
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
    // Nur wo der Tab existiert, sonst meldete die Suche Treffer ohne Zeile.
    ...(hasPermissionsTab
      ? {
          permAx:
            "berechtigungen berechtigung bedienungshilfen accessibility zugriff freigabe erlauben datenschutz tippen geht nicht dialog reparieren",
          permLocation:
            "speicherort programme applications verschieben dmg downloads installieren",
          permDiag:
            "signatur diagnose bundle id pfad fehlerbehebung reparieren problem",
        }
      : {}),
  };

  const q = $derived(navQuery.trim().toLowerCase());
  const hit = (key: string) => q === "" || (KEYWORDS[key] ?? "").includes(q);
  const noMatch = $derived(q !== "" && !Object.keys(KEYWORDS).some(hit));

  /** Zeile sichtbar? Ohne Suche alles, mit Suche nur die Treffer. */
  const show = hit;
  const showSection = (id: SectionId) =>
    section(id)?.keys.some(hit) ?? false;

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

  /** Doppelklick auf einen Slider: zurück auf den Auslieferungs-Default. */
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

  const TYPE_MODES = [
    { label: "Zeichenweise", value: "per_char" },
    { label: "Auf einmal", value: "bulk" },
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
    capturing = capturing === key ? null : key;
    hotkeyTaken = false;
  }

  function onKeydown(event: KeyboardEvent) {
    if (
      event.key === "Escape" &&
      (changelogOpen || helpOpen || confirmClearOpen)
    ) {
      changelogOpen = false;
      helpOpen = false;
      confirmClearOpen = false;
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

<svelte:window onkeydown={onKeydown} />

{#snippet heading(id: SectionId)}
  {@const s = section(id)}
  {#if s}
    <h2 class="sec-title">
      <Icon name={s.icon} size={14} />{s.label}
    </h2>
  {/if}
{/snippet}

{#if settings}
  <main>
    <!-- Hero: die zwei Hotkeys SIND die App — Klick auf die Keycaps nimmt neu auf. -->
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
          Abbrechen
        </span>
      </div>
    </header>

    <div class="body">
      <!-- Seitenleiste: Suche und Sprungmarken; alles steht auf einer Seite. -->
      <nav aria-label="Abschnitte" class="side">
        <label class="search" class:filled={navQuery !== ""}>
          <Icon name="search" size={13} />
          <input
            aria-label="Einstellungen durchsuchen"
            placeholder="Suchen"
            spellcheck="false"
            bind:value={navQuery}
          >
          {#if navQuery !== ""}
            <button
              aria-label="Suche leeren"
              class="clear"
              onclick={() => (navQuery = "")}
              type="button"
            >
              <Icon name="x" size={11} />
            </button>
          {/if}
        </label>
        <ul class:dim={q !== ""}>
          {#each VISIBLE_SECTIONS as s (s.id)}
            <li>
              <button
                aria-current={activeSection === s.id && q === ""
                  ? "true"
                  : undefined}
                class="navitem"
                onclick={() => scrollToSection(s.id)}
                type="button"
                class:active={activeSection === s.id && q === ""}
              >
                <Icon name={s.icon} size={14} />{s.label}
              </button>
            </li>
          {/each}
        </ul>
      </nav>

      <div
        class="pane"
        onscroll={onPaneScroll}
        bind:this={pane}
        class:searching={q !== ""}
      >
        {#if showSection("allgemein")}
          <section id="sec-allgemein">
            {@render heading("allgemein")}
            <div class="card">
              {#if show("theme")}
                <div class="row">
                  <span class="row-label">Darstellung</span>
                  <span class="segmented">
                    {#each THEMES as t (t.value)}
                      <button
                        aria-pressed={settings.theme === t.value}
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
              {#if show("sounds")}
                <SwitchRow
                  label="Töne"
                  onchange={save}
                  bind:checked={settings.sounds}
                />
              {/if}
            </div>
          </section>
        {/if}

        {#if showSection("tippen")}
          <section id="sec-tippen">
            {@render heading("tippen")}
            <div class="card">
              {#if show("typeMode")}
                <div class="row">
                  <span class="row-label">
                    Modus
                    <span class="row-hint">Zeichenweise klappt auch in RDP und Citrix</span>
                  </span>
                  <span class="segmented">
                    {#each TYPE_MODES as m (m.value)}
                      <button
                        aria-pressed={settings.typing.mode === m.value}
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
              {#if show("charDelay") && settings.typing.mode === "per_char"}
                <SliderRow
                  label="Zeichenabstand"
                  max={100}
                  min={1}
                  onchange={save}
                  onreset={() => resetTo("typing", "char_delay_ms")}
                  unit="ms"
                  bind:value={settings.typing.char_delay_ms}
                />
              {/if}
              {#if show("preDelay")}
                <SliderRow
                  label="Startverzögerung"
                  max={3000}
                  min={0}
                  onchange={save}
                  onreset={() => resetTo("typing", "pre_delay_ms")}
                  step={100}
                  unit="ms"
                  bind:value={settings.typing.pre_delay_ms}
                />
              {/if}
              {#if show("trim")}
                <SwitchRow
                  hint="Leerzeichen und Umbrüche am Anfang und Ende"
                  label="Leerraum entfernen"
                  onchange={save}
                  bind:checked={settings.typing.trim}
                />
              {/if}
            </div>
          </section>
        {/if}

        {#if showSection("fenster")}
          <section id="sec-fenster">
            {@render heading("fenster")}
            <div class="card">
              {#if show("winScreen")}
                <div class="row">
                  <label class="row-label" for="window-screen">
                    Öffnen auf
                    <span class="row-hint">Oben am Rand ziehen verschiebt das Fenster</span>
                  </label>
                  <span class="row-actions">
                    {#if settings.history.window_position}
                      <button
                        class="btn"
                        onclick={forgetPosition}
                        title="Verschobene Position vergessen"
                        type="button"
                      >
                        Zurücksetzen
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
              {/if}
              {#if show("winSize")}
                <SliderRow
                  hint="Anteil an der Bildschirmhöhe"
                  label="Größe"
                  max={90}
                  min={40}
                  onchange={save}
                  onreset={() => resetTo("history", "window_size")}
                  step={2}
                  unit="%"
                  bind:value={settings.history.window_size}
                />
              {/if}
              {#if show("closeOnBlur")}
                <SwitchRow
                  label="Bei Klick außerhalb schließen"
                  onchange={save}
                  bind:checked={settings.history.close_on_blur}
                />
              {/if}
            </div>
          </section>
        {/if}

        {#if showSection("erfassen")}
          <section id="sec-erfassen">
            {@render heading("erfassen")}
            <div class="card">
              {#if show("capImages")}
                <SwitchRow
                  label="Bilder"
                  onchange={save}
                  bind:checked={settings.history.capture_images}
                />
              {/if}
              {#if show("capFiles")}
                <SwitchRow
                  label="Dateipfade"
                  onchange={save}
                  bind:checked={settings.history.capture_files}
                />
              {/if}
              {#if show("capHtml")}
                <SwitchRow
                  hint="Getippt wird immer reiner Text"
                  label="Formatierung"
                  onchange={save}
                  bind:checked={settings.history.capture_html}
                />
              {/if}
              {#if show("excludeApps")}
                <div class="row stack">
                  <span class="row-label">
                    Ausgenommene Programme
                    <span class="row-hint">Aus diesen Programmen wird nichts erfasst</span>
                  </span>
                  <div class="tags">
                    {#each settings.history.excluded_apps as app (app)}
                      <button
                        aria-label="{app} entfernen"
                        class="tag"
                        onclick={() => removeExcluded(app)}
                        title="Entfernen"
                        type="button"
                      >
                        {app}<Icon name="x" size={11} />
                      </button>
                    {/each}
                    <input
                      aria-label="Programm hinzufügen"
                      class="tag-input"
                      onblur={addExcluded}
                      onkeydown={(e) => {
                        if (e.key === "Enter") {
                          e.preventDefault();
                          addExcluded();
                        }
                      }}
                      placeholder={settings.history.excluded_apps.length > 0
                        ? "Hinzufügen…"
                        : "z. B. KeePass, dann Enter"}
                      spellcheck="false"
                      type="text"
                      bind:value={excludeInput}
                    >
                  </div>
                </div>
              {/if}
            </div>
          </section>
        {/if}

        {#if showSection("aufbewahrung")}
          <section id="sec-aufbewahrung">
            {@render heading("aufbewahrung")}
            <div class="card">
              {#if show("maxEntries")}
                <SliderRow
                  label="Maximale Einträge"
                  max={5000}
                  min={100}
                  onchange={save}
                  onreset={() => resetTo("history", "max_entries")}
                  step={100}
                  bind:value={settings.history.max_entries}
                />
              {/if}
              {#if show("retention")}
                <div class="row">
                  <label class="row-label" for="retention">
                    Automatisch aufräumen
                    <span class="row-hint">In den Papierkorb, Angepinntes und Bausteine bleiben</span>
                  </label>
                  <Select
                    id="retention"
                    onchange={save}
                    options={RETENTIONS}
                    bind:value={settings.history.retention_days}
                  />
                </div>
              {/if}
              {#if show("clearHistory")}
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
              {/if}
            </div>
          </section>
        {/if}

        {#if showSection("daten")}
          <section id="sec-daten">
            {@render heading("daten")}
            <div class="card">
              {#if show("backup")}
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
              {/if}
              {#if show("dataDir")}
                <div class="row">
                  <span class="row-label">
                    Datenordner
                    <span class="row-hint mono">~/.labi/tippit</span>
                  </span>
                  <button class="btn" onclick={openFolder} type="button">
                    <Icon name="external" size={14} />Öffnen
                  </button>
                </div>
              {/if}
            </div>
          </section>
        {/if}

        {#if hasPermissionsTab && showSection("berechtigungen")}
          <section id="sec-berechtigungen">
            {@render heading("berechtigungen")}
            <PermissionsPane {q} {show} />
          </section>
        {/if}

        {#if noMatch}
          <p class="noresults">Keine Treffer für „{navQuery}“</p>
        {/if}
      </div>
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
    padding: 10px 14px;
    background: var(--bg-sunken);
    border-bottom: 1px solid var(--border);
  }
  .hk {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: 7px;
    align-items: center;
    justify-content: center;
    min-width: 0;
    padding: 9px 8px 8px;
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
  .mark {
    padding: 2px 5px;
    margin: -2px -5px;
    background: var(--mark);
    border-radius: var(--r-sm);
  }

  /* ---- Körper: Seitenleiste links, eine scrollende Seite rechts ---- */
  .body {
    display: flex;
    flex: 1;
    min-height: 0;
  }
  .side {
    display: flex;
    flex: none;
    flex-direction: column;
    gap: 10px;
    width: 176px;
    padding: 12px 10px;
    background: var(--bg-sunken);
    border-right: 1px solid var(--border);
  }
  .side ul {
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 0;
    margin: 0;
    list-style: none;
    transition: opacity var(--t-fast) linear;
  }
  .side ul.dim {
    opacity: 0.4;
  }
  .navitem {
    display: flex;
    gap: 9px;
    align-items: center;
    width: 100%;
    height: 30px;
    padding: 0 10px;
    font: 500 var(--fs-control) / 1 var(--font-ui);
    color: var(--fg-muted);
    text-align: left;
    white-space: nowrap;
    cursor: pointer;
    background: transparent;
    border: 0;
    border-radius: var(--r-md);
    transition:
      background var(--t-fast) linear,
      color var(--t-fast) linear;
  }
  .navitem:hover {
    color: var(--fg);
    background: var(--row-hover);
  }
  .navitem.active {
    color: var(--fg);
    background: var(--bg-raised);
  }
  .navitem:focus-visible {
    outline: none;
    box-shadow: var(--shadow-focus);
  }
  .search {
    display: flex;
    flex: none;
    gap: 6px;
    align-items: center;
    height: 30px;
    padding: 0 8px 0 10px;
    color: var(--fg-dim);
    cursor: text;
    background: var(--bg-raised);
    border-radius: var(--r-md);
  }
  .search input {
    flex: 1;
    min-width: 0;
    padding: 0;
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
  .clear {
    display: grid;
    flex: none;
    place-items: center;
    width: 18px;
    height: 18px;
    padding: 0;
    color: var(--fg-dim);
    cursor: pointer;
    background: var(--bg-strong);
    border: 0;
    border-radius: var(--r-round);
  }

  .pane {
    flex: 1;
    min-width: 0;
    padding: 4px 18px 18px;
    overflow-y: auto;
    scroll-padding-top: 8px;
  }
  section {
    padding-top: 14px;
  }
  /* Auch der letzte Abschnitt lässt sich an den oberen Rand springen. */
  .pane:not(.searching) section:last-of-type {
    min-height: calc(100% - 8px);
  }
  .sec-title {
    display: flex;
    gap: 7px;
    align-items: center;
    margin: 0 0 7px 4px;
    font: 600 var(--fs-micro) / 1 var(--font-ui);
    color: var(--fg-dim);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  .card {
    overflow: hidden;
    background: var(--bg-raised);
    border: 1px solid var(--border-soft);
    border-radius: var(--r-lg);
  }

  .segmented {
    display: inline-flex;
    flex: none;
    gap: 2px;
    padding: 2px;
    background: var(--bg-base);
    border-radius: var(--r-md);
  }
  .segmented button {
    height: 24px;
    padding: 0 11px;
    font: 500 var(--fs-button) / 1 var(--font-ui);
    color: var(--fg-muted);
    cursor: pointer;
    background: transparent;
    border: 0;
    border-radius: calc(var(--r-md) - 2px);
    transition:
      background var(--t-fast) linear,
      color var(--t-fast) linear;
  }
  .segmented button:hover {
    color: var(--fg);
  }
  .segmented button.on {
    color: var(--fg);
    background: var(--bg-strong);
  }
  .segmented button:focus-visible {
    outline: none;
    box-shadow: var(--shadow-focus);
  }

  /* Ausgenommene Programme: Tags mit Eingabe in derselben Zeile. */
  .tags {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    align-items: center;
    width: 100%;
    min-height: 32px;
    padding: 4px;
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
    height: 24px;
    padding: 0 8px 0 10px;
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
    flex: 1 1 140px;
    min-width: 0;
    height: 24px;
    padding: 0 6px;
    font: 400 var(--fs-control) / 1 var(--font-ui);
    color: var(--fg);
    outline: none;
    background: transparent;
    border: 0;
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
  .inline {
    display: flex;
    gap: 6px;
    width: 100%;
  }
  .inline .input {
    flex: 1;
    min-width: 0;
    background: var(--bg-base);
  }
  .err-line {
    display: inline-flex;
    gap: var(--s-2);
    align-items: center;
    font-size: var(--fs-micro);
    color: var(--danger);
  }
  .mono {
    font-family: var(--font-mono);
  }

  .noresults {
    padding-top: 26%;
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
