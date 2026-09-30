<script lang="ts">
  import { onMount, tick } from "svelte";
  import {
    type EntryDto,
    entryText,
    entryThumb,
    getSettings,
    KIND_IMAGE,
    type PaletteModifier,
  } from "$lib/api";
  import { displayPreview, entryMeta, isTotp } from "$lib/entry-kinds";
  import { formatCode } from "$lib/format";
  import Icon from "$lib/icon.svelte";
  import { digitIndex, modifierHeld } from "$lib/palette";
  import {
    onPaletteOpen,
    type PaletteEntry,
    paletteEntries,
    paletteHide,
    palettePick,
    paletteReady,
  } from "$lib/palette-api";
  import { applyWindowChrome } from "$lib/platform";
  import { initTheme } from "$lib/theme";
  import { parseTotp, type TotpConfig, totpNow } from "$lib/totp";
  import "$lib/theme.css";

  let entries = $state<PaletteEntry[]>([]);
  let thumbs = $state<Record<string, string>>({});
  let codes = $state<Record<string, string>>({});
  let typeModifier: PaletteModifier = "shift";
  let totpConfigs = new Map<string, TotpConfig>();
  let main: HTMLElement | undefined = $state();
  let loadId = 0;
  let busy = false;

  /** entryMeta liest nur kind, preview und snippet. */
  const meta = (e: PaletteEntry) => entryMeta(e as unknown as EntryDto);

  async function refreshCodes() {
    const pairs = await Promise.all(
      [...totpConfigs].map(
        async ([uuid, cfg]) => [uuid, (await totpNow(cfg)).code] as const
      )
    );
    codes = Object.fromEntries(pairs);
  }

  /** TOTP-Secrets für den Live-Code; ein nicht lesbares Secret zeigt nur die
      maskierte Vorschau. */
  async function loadTotp(list: PaletteEntry[]) {
    const next = new Map<string, TotpConfig>();
    await Promise.all(
      list.filter(isTotp).map(async (e) => {
        const text = await entryText(e.uuid).catch(() => null);
        const cfg = text ? parseTotp(text) : null;
        if (cfg) {
          next.set(e.uuid, cfg);
        }
      })
    );
    return next;
  }

  async function loadThumbs(list: PaletteEntry[], id: number) {
    const next: Record<string, string> = {};
    await Promise.all(
      list
        .filter((e) => e.kind === KIND_IMAGE && e.has_thumb)
        .map(async (e) => {
          const url = await entryThumb(e.uuid).catch(() => null);
          if (url) {
            next[e.uuid] = url;
          }
        })
    );
    if (id === loadId) {
      thumbs = next;
    }
  }

  /** Einträge laden, rendern, Größe melden; erst dann zeigt das Backend die
      Palette am Mauszeiger. */
  async function load() {
    const id = ++loadId;
    const [list, settings] = await Promise.all([
      paletteEntries(),
      getSettings().catch(() => null),
    ]);
    const configs = await loadTotp(list);
    if (id !== loadId) {
      return;
    }
    if (settings) {
      typeModifier = settings.palette.type_modifier;
    }
    totpConfigs = configs;
    await refreshCodes();
    // Ein neuerer Auslöser hat übernommen: dessen Liste und Größe gelten.
    if (id !== loadId) {
      return;
    }
    entries = list;
    busy = false;
    loadThumbs(list, id).catch(() => undefined);
    await tick();
    if (id !== loadId) {
      return;
    }
    if (main) {
      const rect = main.getBoundingClientRect();
      await paletteReady(Math.ceil(rect.width), Math.ceil(rect.height));
    }
  }

  async function pick(entry: PaletteEntry | undefined, typeChars: boolean) {
    if (!entry || busy) {
      return;
    }
    const cfg = totpConfigs.get(entry.uuid);
    // Ohne Code kein Rückfall auf den Rohtext: der trüge das Secret.
    if (isTotp(entry) && !cfg) {
      return;
    }
    busy = true;
    const code = cfg ? (await totpNow(cfg)).code : undefined;
    await palettePick(entry.uuid, typeChars, code).catch(() => {
      // Rust-Log
    });
    busy = false;
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      paletteHide().catch(() => undefined);
      return;
    }
    const index = digitIndex(event.code, entries.length);
    if (index !== null) {
      event.preventDefault();
      pick(entries[index], modifierHeld(event, typeModifier));
    }
  }

  onMount(() => {
    applyWindowChrome();
    const stopTheme = initTheme();
    const unlisten = onPaletteOpen(() => {
      load().catch(() => undefined);
    });
    load().catch(() => undefined);
    const timer = setInterval(() => {
      if (totpConfigs.size > 0) {
        refreshCodes().catch(() => undefined);
      }
    }, 1000);
    return () => {
      clearInterval(timer);
      unlisten.then((stop) => stop()).catch(() => undefined);
      stopTheme();
    };
  });
</script>

<svelte:window onkeydown={onKeydown} />

<main bind:this={main}>
  {#each entries as entry, i (entry.uuid)}
    {@const m = meta(entry)}
    <button
      class="row"
      onclick={(event) => pick(entry, modifierHeld(event, typeModifier))}
      type="button"
    >
      <span class="num">{i + 1}</span>
      <span class="glyph" style:color={m.colorVar}>
        {#if thumbs[entry.uuid]}
          <img alt="" src={thumbs[entry.uuid]} />
        {:else}
          <Icon name={m.icon} size={14} />
        {/if}
      </span>
      {#if codes[entry.uuid]}
        <span class="text code">{formatCode(codes[entry.uuid])}</span>
      {:else}
        <span class="text">{displayPreview(entry)}</span>
      {/if}
    </button>
  {:else}
    <div class="empty">Keine Einträge</div>
  {/each}
</main>

<style>
  :global(html),
  :global(body) {
    margin: 0;
    overflow: hidden;
    font-family: var(--font-ui);
    user-select: none;
  }
  /* Fenster-Chrome je Plattform, s. history/+page.svelte. */
  :global(html[data-chrome="floating"]),
  :global(html[data-chrome="floating"] body) {
    background: transparent !important;
  }
  /* Breite fest, Höhe nach Inhalt: das Backend übernimmt die gemessene Größe. */
  main {
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    gap: 2px;
    width: 340px;
    padding: 5px;
    font-size: var(--fs-row);
    color: var(--fg-body);
    background: var(--bg-base);
    border: 1px solid var(--border-window);
    border-radius: var(--r-xl);
  }
  :global(html[data-chrome="native"]) main {
    border-color: transparent;
    border-radius: 0;
  }
  .row {
    display: flex;
    gap: 8px;
    align-items: center;
    height: 34px;
    padding: 0 8px;
    font: inherit;
    color: inherit;
    text-align: left;
    cursor: pointer;
    background: transparent;
    border: 0;
    border-radius: var(--r-md);
    transition: background var(--t-fast) linear;
  }
  .row:hover,
  .row:focus-visible {
    outline: none;
    background: var(--row-hover);
  }
  .num {
    flex: none;
    width: 14px;
    font-family: var(--font-mono);
    font-size: var(--fs-micro);
    color: var(--fg-dim);
    text-align: center;
  }
  .glyph {
    display: grid;
    flex: none;
    place-items: center;
    width: 24px;
    height: 24px;
    overflow: hidden;
    border-radius: var(--r-sm);
  }
  .glyph img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .text {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .code {
    font-family: var(--font-mono);
    color: var(--fg);
    letter-spacing: 0.04em;
  }
  .empty {
    display: flex;
    align-items: center;
    height: 34px;
    padding: 0 8px;
    font-size: var(--fs-meta);
    color: var(--fg-muted);
  }
</style>
