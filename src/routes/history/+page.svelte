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
      return "🖼️";
    }
    if (kind === KIND_FILES) {
      return "📁";
    }
    return "📝";
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
    color: #cdd6f4;
    background: #16161f;
    border: 1px solid #313244;
  }
  header {
    padding: 10px 10px 6px;
    border-bottom: 1px solid #27273a;
  }
  .top-row {
    display: flex;
    gap: 6px;
    align-items: center;
  }
  input {
    box-sizing: border-box;
    flex: 1;
    padding: 8px 10px;
    font-size: 13px;
    color: #cdd6f4;
    outline: none;
    background: #1e1e2e;
    border: 1px solid #313244;
    border-radius: 8px;
  }
  input:focus {
    border-color: #89b4fa;
  }
  .close {
    flex: none;
    width: 34px;
    height: 34px;
    font-size: 14px;
    color: #a6adc8;
    cursor: pointer;
    background: #1e1e2e;
    border: 1px solid #313244;
    border-radius: 8px;
  }
  .close:hover {
    color: #f38ba8;
    background: #302030;
    border-color: #45304a;
  }
  nav {
    display: flex;
    gap: 6px;
    margin-top: 8px;
  }
  .chip {
    padding: 3px 12px;
    font-size: 12px;
    color: #a6adc8;
    cursor: pointer;
    background: #1e1e2e;
    border: 1px solid #313244;
    border-radius: 999px;
  }
  .chip.active {
    font-weight: 600;
    color: #11111b;
    background: #89b4fa;
    border-color: #89b4fa;
  }
  section {
    flex: 1;
    min-height: 0;
    padding: 6px;
    overflow-y: auto;
  }
  .row {
    position: relative;
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 7px 8px;
    cursor: default;
    border-radius: 8px;
  }
  .row.selected {
    background: #27273a;
  }
  .icon {
    flex: none;
    font-size: 15px;
  }
  .body {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .preview {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .body img {
    align-self: flex-start;
    max-width: 100%;
    max-height: 44px;
    border-radius: 4px;
  }
  .time {
    font-size: 11px;
    color: #7f849c;
  }
  .actions {
    display: none;
    gap: 2px;
  }
  .row.selected .actions {
    display: flex;
  }
  .actions button {
    padding: 3px 5px;
    font-size: 13px;
    color: #a6adc8;
    cursor: pointer;
    background: transparent;
    border: none;
    border-radius: 5px;
  }
  .actions button:hover {
    color: #cdd6f4;
    background: #313244;
  }
  .actions button.pinned {
    color: #f9e2af;
  }
  .pin-badge {
    position: absolute;
    top: 4px;
    right: 6px;
    font-size: 10px;
    color: #f9e2af;
  }
  .row.selected .pin-badge {
    display: none;
  }
  .empty {
    margin-top: 40px;
    color: #7f849c;
    text-align: center;
  }
  .viewer {
    box-sizing: border-box;
    display: flex;
    flex: none;
    align-items: flex-start;
    justify-content: flex-start;
    height: 170px;
    padding: 10px 12px;
    overflow: auto;
    background: #12121a;
    border-top: 1px solid #27273a;
  }
  .viewer pre {
    margin: 0;
    font-family: "Cascadia Mono", Consolas, monospace;
    font-size: 12px;
    color: #cdd6f4;
    word-break: break-word;
    white-space: pre-wrap;
    user-select: text;
  }
  .viewer img {
    display: block;
    max-width: 100%;
    max-height: 148px;
    margin: auto;
    border-radius: 6px;
  }
  .muted {
    color: #7f849c;
  }
  footer {
    display: flex;
    justify-content: space-between;
    padding: 6px 12px;
    font-size: 11px;
    color: #7f849c;
    border-top: 1px solid #27273a;
  }
</style>
