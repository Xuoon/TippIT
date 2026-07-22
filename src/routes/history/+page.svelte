<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import {
    copyEntry,
    deleteEntry,
    type EntryDto,
    entryText,
    entryThumb,
    hideHistoryWindow,
    KIND_IMAGE,
    onHistoryChanged,
    pinEntry,
    searchHistory,
    typeEntry,
  } from "$lib/api";
  import { entryMeta, entryTintVar, FILTERS } from "$lib/entry-kinds";
  import Icon from "$lib/icon.svelte";
  import { primaryModifierLabel, primaryModifierPressed } from "$lib/platform";
  import { initTheme } from "$lib/theme";
  import "$lib/theme.css";

  let query = $state("");
  let filterId = $state(FILTERS[0].id);
  let entries = $state<EntryDto[]>([]);
  let selected = $state(0);
  let searchInput: HTMLInputElement | undefined = $state();
  let listEl: HTMLElement | undefined = $state();
  let thumbs = $state<Record<string, string>>({});
  const thumbsInflight = new Set<string>();
  let previewText = $state<string | null>(null);
  let previewUuid = "";
  const textCache = new Map<string, string>();

  // Filter-Definitionen (Icons, Backend-kind, clientseitige Verfeinerung)
  // kommen zentral aus $lib/entry-kinds — neue Typen werden nur dort ergänzt.
  const filter = $derived(FILTERS.find((f) => f.id === filterId) ?? FILTERS[0]);

  let refreshSeq = 0;

  async function refresh() {
    // Stale-Guard: überlappende Suchen können out-of-order auflösen —
    // nur die Antwort der jüngsten Anfrage darf die Liste setzen.
    const seq = ++refreshSeq;
    let result = await searchHistory(query, filter.backendKind);
    if (seq !== refreshSeq) {
      return;
    }
    if (filter.refine) {
      result = result.filter(filter.refine);
    }
    entries = result;
    if (selected >= entries.length) {
      selected = Math.max(0, entries.length - 1);
    }
    for (const e of entries) {
      if (
        e.kind === KIND_IMAGE &&
        e.has_thumb &&
        !(e.uuid in thumbs) &&
        !thumbsInflight.has(e.uuid)
      ) {
        thumbsInflight.add(e.uuid);
        entryThumb(e.uuid)
          .then((t) => {
            if (t) {
              thumbs = { ...thumbs, [e.uuid]: t };
            }
          })
          .catch(() => {
            // Fehler landet im Rust-Log
          })
          .finally(() => thumbsInflight.delete(e.uuid));
      }
    }
  }

  onMount(() => {
    const stopTheme = initTheme();
    searchInput?.focus();
    // Verstecktes Fenster nicht bei jeder System-Kopie neu laden —
    // beim nächsten Anzeigen (history-shown) wird ohnehin aufgefrischt.
    const unlistenChanged = onHistoryChanged(() => {
      if (!document.hidden) {
        refresh();
      }
    });
    // Beim Anzeigen (Rust-Event): Suche leeren und fokussieren — jedes Öffnen
    // startet frisch, ohne Suchtext der letzten Sitzung. Das Leeren triggert
    // über das query-$effect auch den Refresh.
    const unlistenShown = listen("history-shown", () => {
      if (query === "") {
        refresh();
      } else {
        query = "";
      }
      filterId = FILTERS[0].id;
      searchInput?.focus();
    });

    const onFocus = () => {
      searchInput?.focus();
      searchInput?.select();
    };
    window.addEventListener("focus", onFocus);
    return () => {
      stopTheme();
      unlistenChanged.then((f) => f());
      unlistenShown.then((f) => f());
      window.removeEventListener("focus", onFocus);
    };
  });

  $effect(() => {
    // Svelte-Dependency-Tracking: Effekt läuft bei jeder Query-/Filter-Änderung.
    // biome-ignore lint/suspicious/noUnusedExpressions: bewusstes $effect-Tracking
    query;
    // biome-ignore lint/suspicious/noUnusedExpressions: bewusstes $effect-Tracking
    filterId;
    selected = 0;
    refresh();
  });

  // Vorschau-Panel: vollen Text des ausgewählten Eintrags nachladen.
  $effect(() => {
    const entry = entries[selected];
    if (!entry) {
      previewText = null;
      previewUuid = "";
      return;
    }
    if (entry.uuid === previewUuid) {
      return;
    }
    previewUuid = entry.uuid;
    if (entry.kind === KIND_IMAGE) {
      previewText = null;
      return;
    }
    // Cache: Inhalte sind pro uuid unveränderlich — beim Durchhovern der Liste
    // nicht jedes Mal neu entschlüsseln.
    const cached = textCache.get(entry.uuid);
    if (cached !== undefined) {
      previewText = cached;
      return;
    }
    const requested = entry.uuid;
    entryText(entry.uuid).then((t) => {
      if (t !== null) {
        if (textCache.size > 100) {
          textCache.clear();
        }
        textCache.set(requested, t);
      }
      if (previewUuid === requested) {
        previewText = t;
      }
    });
  });

  function scrollToSelected() {
    listEl
      ?.querySelector(`[data-idx="${selected}"]`)
      ?.scrollIntoView({ block: "nearest" });
  }

  function cycleFilter(dir: 1 | -1) {
    const idx = FILTERS.findIndex((f) => f.id === filterId);
    filterId = FILTERS[(idx + dir + FILTERS.length) % FILTERS.length].id;
  }

  /** Enter/Doppelklick: kopieren und Fenster schließen. */
  function copyAndClose(uuid: string) {
    copyEntry(uuid).catch(() => {
      // Fehler landet im Rust-Log
    });
    hideHistoryWindow();
  }

  const DIGIT_KEY = /^[1-9]$/;

  /** ⌘1–⌘9 (ClipBook-Muster): Eintrag N direkt kopieren und schließen. */
  function handleDigitShortcut(e: KeyboardEvent): boolean {
    if (!(primaryModifierPressed(e) && DIGIT_KEY.test(e.key))) {
      return false;
    }
    const target = entries[Number(e.key) - 1];
    if (!target) {
      return false;
    }
    e.preventDefault();
    copyAndClose(target.uuid);
    return true;
  }

  /** Sofort-Suche: Lostippen startet die Suche, egal wo der Fokus liegt. */
  function handleTypeToSearch(e: KeyboardEvent): boolean {
    if (
      e.key.length !== 1 ||
      e.ctrlKey ||
      e.altKey ||
      e.metaKey ||
      document.activeElement === searchInput
    ) {
      return false;
    }
    e.preventDefault();
    query += e.key;
    searchInput?.focus();
    return true;
  }

  async function onKeydown(e: KeyboardEvent) {
    const current = entries[selected];
    if (e.key === "Escape") {
      e.preventDefault();
      hideHistoryWindow();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      selected = Math.min(selected + 1, entries.length - 1);
      scrollToSelected();
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      selected = Math.max(selected - 1, 0);
      scrollToSelected();
    } else if (e.key === "Tab") {
      e.preventDefault();
      cycleFilter(e.shiftKey ? -1 : 1);
    } else if (e.key === "Enter" && current) {
      e.preventDefault();
      if (primaryModifierPressed(e)) {
        // typeEntry versteckt das Fenster selbst (Rust-Seite).
        await typeEntry(current.uuid).catch(() => {
          // Fehler landet im Rust-Log
        });
      } else {
        copyAndClose(current.uuid);
      }
    } else if (
      primaryModifierPressed(e) &&
      (e.key === "p" || e.key === "P") &&
      current
    ) {
      e.preventDefault();
      await pinEntry(current.uuid, !current.pinned).catch(() => {
        // Fehler landet im Rust-Log
      });
    } else if (
      e.key === "Delete" &&
      (primaryModifierPressed(e) || e.shiftKey) &&
      current
    ) {
      e.preventDefault();
      await deleteEntry(current.uuid).catch(() => {
        // Fehler landet im Rust-Log
      });
    } else if (!handleDigitShortcut(e)) {
      handleTypeToSearch(e);
    }
  }

  function fmtTime(ms: number): string {
    return new Date(ms).toLocaleString("de-DE", {
      day: "2-digit",
      month: "long",
      year: "numeric",
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    });
  }

  function fmtBytes(n: number): string {
    if (n < 1024) {
      return `${n} B`;
    }
    if (n < 1024 * 1024) {
      return `${(n / 1024).toFixed(1)} KB`;
    }
    return `${(n / (1024 * 1024)).toFixed(1)} MB`;
  }

  const current = $derived(entries[selected]);
</script>

<svelte:window onkeydown={onKeydown} />

<main>
  <!-- Spalte 1: Icon-Leiste mit den Kategorien -->
  <aside class="rail">
    {#each FILTERS as item (item.id)}
      <button
        class="rail-item"
        onclick={() => (filterId = item.id)}
        title={item.label}
        type="button"
        class:active={filterId === item.id}
      >
        <Icon name={item.icon} size={17} />
      </button>
    {/each}
  </aside>

  <!-- Spalte 2: Suche + einzeilige Einträge -->
  <div class="list-col">
    <div class="search">
      <Icon name="search" size={15} />
      <input
        placeholder="Tippen Sie zum Suchen…"
        spellcheck="false"
        type="text"
        bind:this={searchInput}
        bind:value={query}
      >
      <button
        aria-label="Schließen"
        class="close"
        onclick={() => hideHistoryWindow()}
        title="Schließen (Esc)"
        type="button"
      >
        <Icon name="x" size={13} />
      </button>
    </div>

    <section class="list" bind:this={listEl}>
      {#each entries as entry, i (entry.uuid)}
        <div
          aria-selected={i === selected}
          class="row"
          data-idx={i}
          ondblclick={() => copyAndClose(entry.uuid)}
          onmouseenter={() => (selected = i)}
          role="option"
          style="--tint: {entryTintVar(entry)}"
          tabindex="-1"
          class:selected={i === selected}
        >
          <span class="row-ic" style="color: {entryMeta(entry).colorVar}">
            <Icon name={entryMeta(entry).icon} size={13} />
          </span>
          {#if entry.kind === KIND_IMAGE}
            {#if thumbs[entry.uuid]}
              <img alt="Vorschau" class="mini" src={thumbs[entry.uuid]}>
            {:else}
              <span class="preview dim">Bild</span>
            {/if}
          {:else}
            <span class="preview">{entry.preview}</span>
          {/if}
          {#if entry.pinned}
            <span class="pin"><Icon name="star-filled" size={11} /></span>
          {/if}
          {#if i < 9}
            <kbd class="row-kbd">{primaryModifierLabel} {i + 1}</kbd>
          {/if}
        </div>
      {:else}
        <p class="empty">
          {query ? "Nichts gefunden." : "Noch nichts kopiert."}
        </p>
      {/each}
    </section>

    <footer>
      <span class="keys">
        <kbd>↑↓</kbd>
        Navigieren · {entries.length} Einträge
      </span>
      <span class="keys">
        <kbd>↵</kbd>
        Kopieren ·
        <kbd>{primaryModifierLabel}+↵</kbd>
        Tippen
      </span>
    </footer>
  </div>

  <!-- Spalte 3: Vorschau + Metadaten -->
  <aside class="detail">
    {#if current}
      <div class="detail-bar">
        <button
          class="act"
          onclick={() => copyEntry(current.uuid)}
          title="Kopieren (Enter)"
          type="button"
        >
          <Icon name="copy" size={15} />
        </button>
        {#if current.kind !== KIND_IMAGE}
          <button
            class="act"
            onclick={() => typeEntry(current.uuid)}
            title="Tippen ({primaryModifierLabel}+Enter)"
            type="button"
          >
            <Icon name="keyboard" size={15} />
          </button>
        {/if}
        <span class="spacer"></span>
        <button
          class="act"
          onclick={() => pinEntry(current.uuid, !current.pinned)}
          title={current.pinned
            ? `Pin lösen (${primaryModifierLabel}+P)`
            : `Anpinnen (${primaryModifierLabel}+P)`}
          type="button"
          class:pinned={current.pinned}
        >
          <Icon name={current.pinned ? "star-filled" : "star"} size={15} />
        </button>
        <button
          class="act danger"
          onclick={() => deleteEntry(current.uuid)}
          title="Löschen ({primaryModifierLabel}+Entf)"
          type="button"
        >
          <Icon name="trash" size={15} />
        </button>
      </div>

      <div class="viewer" class:image={current.kind === KIND_IMAGE}>
        {#if current.kind === KIND_IMAGE}
          {#if thumbs[current.uuid]}
            <img alt="Bildvorschau" src={thumbs[current.uuid]}>
          {:else}
            <span class="muted">Bild wird geladen…</span>
          {/if}
        {:else if previewText !== null}
          <pre>{previewText}</pre>
        {:else}
          <span class="muted">…</span>
        {/if}
      </div>

      <div class="meta">
        <div class="meta-row">
          <span class="meta-label">Typ</span>
          <span class="meta-value">{entryMeta(current).label}</span>
        </div>
        <div class="meta-row">
          <span class="meta-label">Größe</span>
          <span class="meta-value">{fmtBytes(current.size_bytes)}</span>
        </div>
        <div class="meta-row">
          <span class="meta-label">Kopierzeit</span>
          <span class="meta-value">{fmtTime(current.created_at)}</span>
        </div>
      </div>
    {:else}
      <div class="viewer center">
        <span class="muted">Kein Eintrag ausgewählt</span>
      </div>
    {/if}
  </aside>
</main>

<style>
  :global(body) {
    margin: 0;
    overflow: hidden;
    font-family: var(--font-ui);
    user-select: none;
  }
  main {
    display: grid;
    grid-template-columns: 48px 340px 1fr;
    height: 100vh;
    overflow: hidden;
    font-size: var(--fs-row);
    color: var(--fg-body);
    background: var(--bg-base);
    border: 1px solid var(--border-window);
    border-radius: var(--r-xl);
  }

  /* ---- Spalte 1: Rail ---- */
  .rail {
    display: flex;
    flex-direction: column;
    gap: 4px;
    align-items: center;
    padding: 10px 0;
    background: var(--bg-base);
    border-right: 1px solid var(--border);
  }
  .rail-item {
    display: grid;
    place-items: center;
    width: 34px;
    height: 34px;
    color: var(--fg-dim);
    cursor: pointer;
    background: transparent;
    border: 0;
    border-radius: var(--r-lg);
    transition:
      background var(--t-fast) linear,
      color var(--t-fast) linear;
  }
  .rail-item:hover {
    color: var(--fg-body);
    background: var(--row-hover);
  }
  .rail-item.active {
    color: var(--accent-text);
    background: var(--accent-soft);
  }

  /* ---- Spalte 2: Liste ---- */
  .list-col {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    background: var(--bg-base);
    border-right: 1px solid var(--border);
  }
  .search {
    display: flex;
    flex: none;
    gap: 8px;
    align-items: center;
    height: 48px;
    padding: 0 8px 0 12px;
    color: var(--fg-dim);
    border-bottom: 1px solid var(--border);
  }
  .search input {
    flex: 1;
    min-width: 0;
    font-size: var(--fs-search);
    color: var(--fg);
    outline: none;
    background: transparent;
    border: 0;
  }
  .search input::placeholder {
    color: var(--fg-placeholder);
  }
  .close {
    display: grid;
    flex: none;
    place-items: center;
    width: 26px;
    height: 26px;
    color: var(--fg-dim);
    cursor: pointer;
    background: transparent;
    border: 0;
    border-radius: var(--r-md);
  }
  .close:hover {
    color: var(--danger);
    background: var(--row-hover);
  }

  .list {
    flex: 1;
    min-height: 0;
    padding: 6px;
    overflow-y: auto;
  }
  .row {
    display: flex;
    gap: 8px;
    align-items: center;
    height: 34px;
    padding: 0 10px;
    cursor: default;
    /* Leichte Typ-Tönung (--tint kommt pro Zeile aus entry-kinds). */
    background: var(--tint);
    border-radius: var(--r-lg);
  }
  .row + .row {
    margin-top: 2px;
  }
  .row.selected {
    background: var(--row-selected);
    box-shadow: inset 2px 0 0 var(--accent);
  }
  .preview {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--fg-body);
    white-space: nowrap;
  }
  .row.selected .preview {
    color: var(--fg);
  }
  .preview.dim {
    color: var(--fg-dim);
  }
  .mini {
    flex: 1;
    align-self: center;
    max-width: 120px;
    max-height: 24px;
    object-fit: contain;
    object-position: left;
    border-radius: var(--r-sm);
  }
  .pin {
    flex: none;
    color: var(--pin);
  }
  .row-ic {
    display: grid;
    flex: none;
    place-items: center;
    opacity: 0.85;
  }
  .row-kbd {
    flex: none;
    padding: 1px 5px;
    font-size: 10px;
    color: var(--fg-dim);
    white-space: nowrap;
    background: var(--bg-strong);
    border-radius: var(--r-sm);
  }
  .empty {
    margin-top: 48px;
    color: var(--fg-dim);
    text-align: center;
  }

  footer {
    display: flex;
    flex: none;
    gap: 12px;
    align-items: center;
    justify-content: space-between;
    height: 32px;
    padding: 0 12px;
    font-size: var(--fs-micro);
    color: var(--fg-dim);
    border-top: 1px solid var(--border);
  }
  .keys {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  kbd {
    padding: 1px 5px;
    font-size: 10px;
    color: var(--fg-muted);
    background: var(--bg-strong);
    border-radius: var(--r-sm);
  }

  /* ---- Spalte 3: Vorschau ---- */
  .detail {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    background: var(--bg-sunken);
  }
  .detail-bar {
    display: flex;
    flex: none;
    gap: 4px;
    align-items: center;
    height: 44px;
    padding: 0 10px;
    border-bottom: 1px solid var(--border);
  }
  .spacer {
    flex: 1;
  }
  .act {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    color: var(--fg-dim);
    cursor: pointer;
    background: transparent;
    border: 0;
    border-radius: var(--r-md);
    transition:
      background var(--t-fast) linear,
      color var(--t-fast) linear;
  }
  .act:hover {
    color: var(--fg);
    background: var(--row-hover);
  }
  .act.pinned {
    color: var(--pin);
  }
  .act.danger:hover {
    color: var(--danger);
    background: var(--danger-soft);
  }

  .viewer {
    flex: 1;
    min-height: 0;
    padding: 14px 16px;
    overflow: auto;
  }
  .viewer.center,
  .viewer.image {
    display: grid;
    place-items: center;
  }
  .viewer pre {
    margin: 0;
    font-family: var(--font-mono);
    font-size: var(--fs-control);
    line-height: 1.55;
    color: var(--fg-body);
    word-break: break-word;
    white-space: pre-wrap;
    user-select: text;
  }
  /* Bilder mittig und maximiert — nutzen den ganzen Vorschaubereich. */
  .viewer img {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: contain;
    border-radius: var(--r-sm);
  }
  .muted {
    color: var(--fg-dim);
  }

  .meta {
    flex: none;
    padding: 4px 16px 10px;
    border-top: 1px solid var(--border);
  }
  .meta-row {
    display: flex;
    gap: 16px;
    align-items: baseline;
    justify-content: space-between;
    padding: 6px 0;
  }
  .meta-row + .meta-row {
    border-top: 1px solid var(--border-soft);
  }
  .meta-label {
    flex: none;
    font-size: var(--fs-meta);
    color: var(--fg-dim);
  }
  .meta-value {
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: var(--fs-meta);
    color: var(--fg);
    white-space: nowrap;
  }
</style>
