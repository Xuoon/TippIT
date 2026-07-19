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
    KIND_FILES,
    KIND_IMAGE,
    onHistoryChanged,
    pinEntry,
    searchHistory,
    typeEntry,
  } from "$lib/api";

  let query = $state("");
  let kindFilter = $state<number | null>(null);
  let entries = $state<EntryDto[]>([]);
  let selected = $state(0);
  let searchInput: HTMLInputElement | undefined = $state();
  let listEl: HTMLElement | undefined = $state();
  let thumbs = $state<Record<string, string>>({});
  const thumbsInflight = new Set<string>();
  let previewText = $state<string | null>(null);
  let previewUuid = "";
  const textCache = new Map<string, string>();

  const filters: { label: string; kind: number | null }[] = [
    { label: "Alle", kind: null },
    { label: "Text", kind: 0 },
    { label: "Bilder", kind: 1 },
    { label: "Dateien", kind: 2 },
  ];

  let refreshSeq = 0;

  async function refresh() {
    // Stale-Guard: überlappende Suchen können out-of-order auflösen —
    // nur die Antwort der jüngsten Anfrage darf die Liste setzen.
    const seq = ++refreshSeq;
    const result = await searchHistory(query, kindFilter);
    if (seq !== refreshSeq) {
      return;
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
    searchInput?.focus();
    // Verstecktes Fenster nicht bei jeder System-Kopie neu laden —
    // beim nächsten Anzeigen (history-shown) wird ohnehin aufgefrischt.
    const unlistenChanged = onHistoryChanged(() => {
      if (!document.hidden) {
        refresh();
      }
    });
    // Das Fenster öffnet ohne Aktivierung (Fokus bleibt in der bisherigen App) —
    // das Rust-Event ersetzt daher den window-focus-Trigger.
    const unlistenShown = listen("history-shown", () => {
      refresh();
      searchInput?.focus();
      searchInput?.select();
    });

    const onFocus = () => {
      searchInput?.focus();
      searchInput?.select();
    };
    window.addEventListener("focus", onFocus);
    return () => {
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
    kindFilter;
    selected = 0;
    refresh();
  });

  // Vorschau-Panel: vollen Text des ausgewählten Eintrags nachladen.
  $effect(() => {
    const current = entries[selected];
    if (!current) {
      previewText = null;
      previewUuid = "";
      return;
    }
    if (current.uuid === previewUuid) {
      return;
    }
    previewUuid = current.uuid;
    if (current.kind === KIND_IMAGE) {
      previewText = null;
      return;
    }
    // Cache: Inhalte sind pro uuid unveränderlich — beim Durchhovern der Liste
    // nicht jedes Mal neu entschlüsseln.
    const cached = textCache.get(current.uuid);
    if (cached !== undefined) {
      previewText = cached;
      return;
    }
    const requested = current.uuid;
    entryText(current.uuid).then((t) => {
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
    const idx = filters.findIndex((f) => f.kind === kindFilter);
    const next = (idx + dir + filters.length) % filters.length;
    kindFilter = filters[next].kind;
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
      if (e.ctrlKey) {
        await typeEntry(current.uuid).catch(() => {
          // Fehler landet im Rust-Log
        });
      } else {
        await copyEntry(current.uuid).catch(() => {
          // Fehler landet im Rust-Log
        });
      }
    } else if (e.ctrlKey && (e.key === "p" || e.key === "P") && current) {
      e.preventDefault();
      await pinEntry(current.uuid, !current.pinned).catch(() => {
        // Fehler landet im Rust-Log
      });
    } else if (e.key === "Delete" && (e.ctrlKey || e.shiftKey) && current) {
      e.preventDefault();
      await deleteEntry(current.uuid).catch(() => {
        // Fehler landet im Rust-Log
      });
    }
  }

  function fmtTime(ms: number): string {
    const d = new Date(ms);
    const today = new Date();
    const time = d.toLocaleTimeString("de-DE");
    if (d.toDateString() === today.toDateString()) {
      return time;
    }
    return `${d.toLocaleDateString("de-DE", { day: "2-digit", month: "2-digit" })} ${time}`;
  }

  function kindIcon(kind: number): string {
    if (kind === KIND_IMAGE) {
      return "IMG";
    }
    if (kind === KIND_FILES) {
      return "DATEI";
    }
    return "TEXT";
  }

  let current = $derived(entries[selected]);
</script>

<svelte:window onkeydown={onKeydown} />

<main>
  <header>
    <div class="top-row">
      <input
        placeholder="Suchen…  (↑↓ wählen · Enter kopieren · Strg+Enter tippen)"
        spellcheck="false"
        type="text"
        bind:this={searchInput}
        bind:value={query}
      >
      <button
        class="close"
        onclick={() => hideHistoryWindow()}
        title="Schließen (Esc)"
        type="button"
      >
        ✕
      </button>
    </div>
    <nav>
      {#each filters as f (f.label)}
        <button
          class="chip"
          onclick={() => (kindFilter = f.kind)}
          type="button"
          class:active={kindFilter === f.kind}
        >
          {f.label}
        </button>
      {/each}
    </nav>
  </header>

  <section bind:this={listEl}>
    {#each entries as entry, i (entry.uuid)}
      <div
        aria-selected={i === selected}
        class="row"
        data-idx={i}
        ondblclick={() => copyEntry(entry.uuid)}
        onmouseenter={() => (selected = i)}
        role="option"
        tabindex="-1"
        class:selected={i === selected}
      >
        <span class="icon">{kindIcon(entry.kind)}</span>
        <div class="body">
          {#if entry.kind === KIND_IMAGE && thumbs[entry.uuid]}
            <img alt="Vorschau" src={thumbs[entry.uuid]}>
          {:else}
            <span class="preview">{entry.preview}</span>
          {/if}
          <span class="time">{fmtTime(entry.created_at)}</span>
        </div>
        <div class="actions">
          <button
            onclick={() => pinEntry(entry.uuid, !entry.pinned)}
            title={entry.pinned ? "Pin lösen" : "Anpinnen (Strg+P)"}
            type="button"
            class:pinned={entry.pinned}
          >
            ★
          </button>
          <button
            onclick={() => copyEntry(entry.uuid)}
            title="Kopieren (Enter)"
            type="button"
          >
            ⧉
          </button>
          {#if entry.kind !== KIND_IMAGE}
            <button
              onclick={() => typeEntry(entry.uuid)}
              title="Als Tastatur tippen (Strg+Enter)"
              type="button"
            >
              ⌨
            </button>
          {/if}
          <button
            onclick={() => deleteEntry(entry.uuid)}
            title="Löschen (Strg+Entf)"
            type="button"
          >
            ✕
          </button>
        </div>
        {#if entry.pinned}
          <span class="pin-badge">★</span>
        {/if}
      </div>
    {:else}
      <p class="empty">
        {query ? "Nichts gefunden." : "Noch nichts kopiert."}
      </p>
    {/each}
  </section>

  {#if current}
    <aside class="viewer">
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
    </aside>
  {/if}

  <footer>
    <span>Esc/✕ schließen</span>
    <span>Tab: Filter</span>
    <span>{entries.length} Einträge</span>
  </footer>
</main>

<style>
  :global(body) {
    margin: 0;
    overflow: hidden;
    user-select: none;
  }
  main {
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    height: 100vh;
    font-family: "Segoe UI", system-ui, sans-serif;
    font-size: 13px;
    color: #d9e0ef;
    background: #151821;
    border: 1px solid #343946;
  }
  header {
    padding: 9px 12px 0;
    border-bottom: 1px solid #2a2f3a;
  }
  .top-row {
    display: flex;
    gap: 6px;
    align-items: center;
  }
  input {
    box-sizing: border-box;
    flex: 1;
    padding: 7px 2px;
    font-size: 13px;
    color: #d9e0ef;
    outline: none;
    background: transparent;
    border: 0;
    border-bottom: 1px solid #353b49;
    border-radius: 0;
  }
  input:focus {
    border-color: #78a7ec;
  }
  .close {
    flex: none;
    width: 28px;
    height: 28px;
    font-size: 15px;
    color: #7f899e;
    cursor: pointer;
    background: transparent;
    border: 0;
  }
  .close:hover {
    color: #df93a3;
    background: transparent;
  }
  nav {
    display: flex;
    gap: 18px;
    margin-top: 6px;
  }
  .chip {
    padding: 6px 0 7px;
    font-size: 12px;
    color: #7f899e;
    cursor: pointer;
    background: transparent;
    border: 0;
    border-bottom: 2px solid transparent;
  }
  .chip.active {
    font-weight: 600;
    color: #a9c8f6;
    background: transparent;
    border-color: #78a7ec;
  }
  section {
    flex: 1;
    min-height: 0;
    padding: 0 10px;
    overflow-y: auto;
  }
  .row {
    position: relative;
    display: flex;
    gap: 9px;
    align-items: center;
    min-height: 42px;
    padding: 5px 2px;
    cursor: default;
    border-bottom: 1px solid #222732;
  }
  .row.selected {
    background: #1c212b;
    box-shadow: inset 2px 0 #709bd9;
  }
  .icon {
    flex: none;
    width: 34px;
    font-size: 9px;
    font-weight: 650;
    color: #77849a;
    letter-spacing: 0.03em;
  }
  .body {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }
  .preview {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .body img {
    align-self: flex-start;
    max-width: 72px;
    max-height: 34px;
    border-radius: 2px;
  }
  .time {
    font-size: 11px;
    color: #707a8e;
  }
  .actions {
    display: none;
    gap: 1px;
  }
  .row.selected .actions {
    display: flex;
  }
  .actions button {
    padding: 3px 5px;
    font-size: 13px;
    color: #8490a5;
    cursor: pointer;
    background: transparent;
    border: none;
    border-radius: 2px;
  }
  .actions button:hover {
    color: #d9e0ef;
    background: #2b313e;
  }
  .actions button.pinned {
    color: #d9bb76;
  }
  .pin-badge {
    position: absolute;
    top: 4px;
    right: 6px;
    font-size: 10px;
    color: #d9bb76;
  }
  .row.selected .pin-badge {
    display: none;
  }
  .empty {
    margin-top: 40px;
    color: #707a8e;
    text-align: center;
  }
  .viewer {
    box-sizing: border-box;
    display: flex;
    flex: none;
    align-items: flex-start;
    justify-content: flex-start;
    height: 132px;
    padding: 9px 12px;
    overflow: auto;
    background: #11141b;
    border-top: 1px solid #2a2f3a;
  }
  .viewer pre {
    margin: 0;
    font-family: "Cascadia Mono", Consolas, monospace;
    font-size: 12px;
    color: #cfd7e8;
    word-break: break-word;
    white-space: pre-wrap;
    user-select: text;
  }
  .viewer img {
    display: block;
    max-width: 100%;
    max-height: 112px;
    margin: auto;
    border-radius: 2px;
  }
  .muted {
    color: #707a8e;
  }
  footer {
    display: flex;
    justify-content: space-between;
    padding: 5px 12px;
    font-size: 11px;
    color: #697386;
    border-top: 1px solid #2a2f3a;
  }
</style>
