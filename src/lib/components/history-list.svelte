<script lang="ts">
  import { type EntryDto, KIND_IMAGE } from "$lib/api";
  import { entryMeta, isTotp, type SortKey } from "$lib/entry-kinds";
  import { dateGroupLabel } from "$lib/history/grouping";
  import Icon from "$lib/icon.svelte";
  import { markMatches } from "$lib/preview";
  import { maskOtpauthSecret } from "$lib/totp";

  interface Props {
    emptyMessage: string;
    entries: EntryDto[];
    /** Datums-Zwischenüberschriften einblenden. */
    grouping: boolean;
    /** id der Liste, auf die das Suchfeld per aria-controls zeigt. */
    listId: string;
    /** Zeile will ihr Thumbnail (nur Einträge im Sichtfenster). */
    onneedthumb: (entry: EntryDto) => void;
    onpick: (entry: EntryDto, event: MouseEvent) => void;
    onpointerselect: (index: number, event: MouseEvent) => void;
    query: string;
    selected: number;
    sortKey: SortKey;
    thumbs: Record<string, string>;
  }

  const {
    entries,
    selected,
    grouping,
    sortKey,
    query,
    thumbs,
    emptyMessage,
    listId,
    onpick,
    onpointerselect,
    onneedthumb,
  }: Props = $props();

  // Höhen sind fix: ROW_H entspricht --row-h aus theme.css, HEAD_H der
  // .group-head-Regel unten. Nur deshalb lässt sich die Position jeder Zeile
  // ohne Messung ausrechnen, und nur deshalb bleibt die Liste auch bei
  // tausenden Einträgen flüssig.
  const ROW_H = 34;
  const HEAD_H = 26;
  /** Zeilen über und unter dem Sichtfenster, damit Scrollen nicht flackert. */
  const OVERSCAN = 8;

  let listEl: HTMLElement | undefined = $state();
  let scrollTop = $state(0);
  let viewportH = $state(600);

  interface ListItem {
    entry?: EntryDto;
    idx: number;
    label?: string;
    offset: number;
    type: "head" | "row";
  }

  /** Flache Liste aus Zeilen und (optionalen) Datums-Zwischenüberschriften. */
  const listItems = $derived.by(() => {
    const items: ListItem[] = [];
    let offset = 0;
    let lastLabel = "";
    entries.forEach((entry, idx) => {
      if (grouping) {
        const label = dateGroupLabel(entry, sortKey);
        if (label !== lastLabel) {
          items.push({ type: "head", label, idx, offset });
          offset += HEAD_H;
          lastLabel = label;
        }
      }
      items.push({ type: "row", entry, idx, offset });
      offset += ROW_H;
    });
    return items;
  });

  const listHeight = $derived(
    listItems.at(-1)
      ? (listItems.at(-1)?.offset ?? 0) +
          (listItems.at(-1)?.type === "head" ? HEAD_H : ROW_H)
      : 0
  );

  const visible = $derived.by(() => {
    const top = Math.max(0, scrollTop - OVERSCAN * ROW_H);
    const bottom = scrollTop + viewportH + OVERSCAN * ROW_H;
    let start = listItems.findIndex((it) => it.offset >= top);
    if (start === -1) {
      start = Math.max(0, listItems.length - 1);
    }
    let end = listItems.findIndex((it) => it.offset > bottom);
    if (end === -1) {
      end = listItems.length;
    }
    const slice = listItems.slice(start, end);
    // Steht oben eine Zeile ohne ihre Überschrift, wird deren Überschrift
    // vorangestellt — sonst hätte die klebende Kopfzeile nichts zum Kleben.
    const first = slice[0];
    const needsHead = grouping && first?.type === "row";
    return {
      items: slice,
      needsHead,
      headLabel:
        needsHead && first?.entry ? dateGroupLabel(first.entry, sortKey) : "",
      padTop: (first?.offset ?? 0) - (needsHead ? HEAD_H : 0),
    };
  });

  // Thumbnails nur für das gerenderte Sichtfenster: die Liste ist
  // virtualisiert, und eine leere Suche liefert die ganze Historie.
  $effect(() => {
    for (const it of visible.items) {
      if (it.type === "row" && it.entry) {
        onneedthumb(it.entry);
      }
    }
  });

  function onListScroll(e: Event) {
    scrollTop = (e.currentTarget as HTMLElement).scrollTop;
  }

  $effect(() => {
    if (!listEl) {
      return;
    }
    const el = listEl;
    const measure = () => {
      viewportH = el.clientHeight;
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  });

  /** Ausgewählte Zeile ins Sichtfenster holen — rechnerisch, weil die Zeile
      bei virtualisierter Liste gar nicht im DOM sein muss. */
  export function scrollToSelected(): void {
    if (!listEl) {
      return;
    }
    const item = listItems.find(
      (it) => it.type === "row" && it.idx === selected
    );
    if (!item) {
      return;
    }
    const top = item.offset;
    const bottom = top + ROW_H;
    if (top < listEl.scrollTop) {
      listEl.scrollTop = Math.max(0, top - (grouping ? HEAD_H : 0));
    } else if (bottom > listEl.scrollTop + listEl.clientHeight) {
      listEl.scrollTop = bottom - listEl.clientHeight;
    }
  }

  export function resetScroll(): void {
    scrollTop = 0;
    if (listEl) {
      listEl.scrollTop = 0;
    }
  }

  /** Listentext; bei otpauth-URIs ohne sichtbares Secret. */
  function rowText(entry: EntryDto): string {
    return isTotp(entry) ? maskOtpauthSecret(entry.preview) : entry.preview;
  }
</script>

<!-- Virtualisiert: gerendert wird nur das Sichtfenster, die Gesamthöhe
     stellt ein Abstandshalter her. -->
<section class="list" onscroll={onListScroll} bind:this={listEl}>
  {#if entries.length === 0}
    <p class="empty">{emptyMessage}</p>
  {:else}
    <div
      aria-label="Einträge"
      class="list-inner"
      id={listId}
      role="listbox"
      style="height: {listHeight}px"
      tabindex="-1"
    >
      <div role="presentation" style="height: {visible.padTop}px"></div>
      {#if visible.needsHead}
        <div class="group-head" role="presentation">{visible.headLabel}</div>
      {/if}
      {#each visible.items as item (item.type + item.idx)}
        {#if item.type === "head"}
          <div class="group-head" role="presentation">{item.label}</div>
        {:else if item.entry}
          {@const entry = item.entry}
          <div
            aria-selected={item.idx === selected}
            class="row"
            id="e-{entry.uuid}"
            ondblclick={(event) => onpick(entry, event)}
            onmousemove={(event) => onpointerselect(item.idx, event)}
            role="option"
            tabindex="-1"
            class:selected={item.idx === selected}
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
              <!-- Suchtreffer werden markiert; der Text selbst wird in
                   markMatches escaped, eingesetzt werden nur <mark>-Tags. -->
              <span class="preview"
                >{@html markMatches(rowText(entry), query)}</span
              >
            {/if}
            {#if entry.snippet}
              <span class="pin snip"><Icon name="bookmark" size={11} /></span>
            {:else if entry.pinned}
              <span class="pin"><Icon name="star-filled" size={11} /></span>
            {/if}
          </div>
        {/if}
      {/each}
    </div>
  {/if}
</section>

<style>
  .list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }
  .list-inner {
    position: relative;
  }
  /* Datumsgruppen (optional): klebende Versal-Header zwischen den Zeilen. */
  .group-head {
    position: sticky;
    top: 0;
    z-index: 1;
    /* Höhe FIX: HEAD_H im Script rechnet mit genau diesen 26px. */
    box-sizing: border-box;
    height: 26px;
    padding: 7px var(--s-6) 4px;
    font: 600 var(--fs-micro) / 1 var(--font-ui);
    color: var(--fg-dim);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    background: var(--bg-base);
    border-bottom: 1px solid var(--border-soft);
  }
  /* Kompakte Zeilen ohne Trennlinien; die Auswahl ist eine eingerückte,
     leicht gerundete Fläche. Höhe FIX (ROW_H im Script). */
  .row {
    display: flex;
    gap: var(--s-4);
    align-items: center;
    height: var(--row-h);
    padding: 0 var(--s-4);
    margin: 0 var(--s-3);
    cursor: default;
    border-radius: var(--r-sm);
  }
  .row.selected {
    background: var(--bg-hover);
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
  /* In der Zeile trägt nur die Schriftfarbe den Treffer — eine Fläche pro
     Zeile würde die Liste zerhacken (--highlight ist genau dafür da). */
  .preview :global(mark) {
    font-weight: 600;
    color: var(--highlight);
    background: transparent;
  }
  .mini {
    flex: 1;
    align-self: center;
    max-width: 120px;
    max-height: 24px;
    object-fit: contain;
    object-position: left;
    border-radius: var(--r-xs);
  }
  .pin {
    flex: none;
    /* Immer rechtsbündig — Bild-Thumbnails wachsen anders als Text-Previews. */
    margin-left: auto;
    color: var(--pin);
  }
  .pin.snip {
    color: var(--kind-snippet);
  }
  .row-ic {
    display: grid;
    flex: none;
    place-items: center;
    opacity: 0.85;
  }
  .empty {
    margin-top: 48px;
    color: var(--fg-dim);
    text-align: center;
  }
</style>
