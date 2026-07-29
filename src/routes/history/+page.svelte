<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import {
    copyEntry,
    copyText,
    deleteEntry,
    type EntryDto,
    entryImage,
    entryText,
    entryThumb,
    hideHistoryWindow,
    historyTargetApp,
    KIND_IMAGE,
    type OcrBlock,
    ocrEntry,
    onHistoryChanged,
    openEntry,
    openLink,
    pinEntry,
    qrEntry,
    searchHistory,
    sourceAppIcon,
    type TargetAppDto,
    typeEntry,
    typeText,
  } from "$lib/api";
  import {
    type EntryAction,
    entryMeta,
    FILTERS,
    isLink,
    isTotp,
    primaryAction,
    type SortKey,
    sortEntries,
  } from "$lib/entry-kinds";
  import Icon from "$lib/icon.svelte";
  import { primaryModifierLabel, primaryModifierPressed } from "$lib/platform";
  import { initTheme } from "$lib/theme";
  import "$lib/theme.css";
  import { parseTotp, type TotpNow, totpNow } from "$lib/totp";

  const LS_LIST_W = "tippit.history.listW";
  const LS_PREVIEW = "tippit.history.showPreview";
  const LS_META = "tippit.history.showMeta";
  const LS_SORT = "tippit.history.sortKey";
  const LS_SORT_REV = "tippit.history.sortRev";
  const LS_GROUP = "tippit.history.groupByDate";

  let query = $state("");
  let filterId = $state(FILTERS[0].id);
  let rawEntries = $state<EntryDto[]>([]);
  let selected = $state(0);
  let searchInput: HTMLInputElement | undefined = $state();
  let listEl: HTMLElement | undefined = $state();
  let thumbs = $state<Record<string, string>>({});
  const thumbsInflight = new Set<string>();
  let previewText = $state<string | null>(null);
  let previewUuid = "";
  const textCache = new Map<string, string>();
  // Volles Bild für die Vorschau (das Thumbnail ist für die Vollbreite zu
  // grob); lazy nur für den ausgewählten Eintrag, Thumbnail bleibt Fallback.
  let fullImage = $state<string | null>(null);
  let fullImageUuid = "";
  const fullImageCache = new Map<string, string>();
  let appIcons = $state<Record<string, string>>({});
  const appIconsInflight = new Set<string>();

  let listW = $state(340);
  let showPreview = $state(true);
  let showMeta = $state(true);
  let sortKey = $state<SortKey>("last_copy");
  let sortReverse = $state(false);
  let sortOpen = $state(false);
  let groupByDate = $state(false);
  let targetApp = $state<TargetAppDto | null>(null);
  let targetIcon = $state<string | null>(null);
  let hoverAnchor: { x: number; y: number } | null = null;
  let hoverSelectionEnabled = false;

  let ocrBusy = $state(false);
  let ocrText = $state<string | null>(null);
  let ocrBlocks = $state<OcrBlock[]>([]);
  let ocrUuid = "";
  let ocrError = $state("");
  let ocrSeq = 0;

  // QR: dekodierte Inhalte aller lesbaren Codes im Bild (null = noch nicht gelesen).
  let qrBusy = $state(false);
  let qrResults = $state<string[] | null>(null);
  let qrError = $state("");
  let qrSeq = 0;
  const QR_LINK_RE = /^https?:\/\//i;
  const QR_OTPAUTH_RE = /^otpauth:\/\//i;
  // Gerenderte Bildbox (px) fürs Overlay — die object-fit:contain-Skalierung
  // ist ohne Messung nicht in CSS abbildbar, die Boxen aus Vision sind aber
  // normalisiert und mappen so exakt auf die sichtbaren Glyphen.
  let previewImg = $state<HTMLImageElement | null>(null);
  let ocrBox = $state({ left: 0, top: 0, width: 0, height: 0 });

  // TOTP: aus dem entschlüsselten Text (Secret/otpauth) live erzeugter Code.
  let totp = $state<TotpNow | null>(null);

  function measureOcrBox() {
    if (previewImg) {
      ocrBox = {
        left: previewImg.offsetLeft,
        top: previewImg.offsetTop,
        width: previewImg.offsetWidth,
        height: previewImg.offsetHeight,
      };
    }
  }

  $effect(() => {
    if (!previewImg || ocrBlocks.length === 0) {
      return;
    }
    measureOcrBox();
    const ro = new ResizeObserver(measureOcrBox);
    ro.observe(previewImg);
    return () => ro.disconnect();
  });

  const filter = $derived(FILTERS.find((f) => f.id === filterId) ?? FILTERS[0]);
  const entries = $derived(sortEntries(rawEntries, sortKey, sortReverse));
  const current = $derived(entries[selected]);
  const currentIsTotp = $derived(!!current && isTotp(current));
  const currentAction = $derived<EntryAction | null>(
    current ? primaryAction(current) : null
  );
  const currentLink = $derived(
    current && isLink(current) ? (previewText ?? current.preview) : null
  );

  let refreshSeq = 0;

  async function refresh() {
    const seq = ++refreshSeq;
    let result = await searchHistory(query, filter.backendKind);
    if (seq !== refreshSeq) {
      return;
    }
    if (filter.refine) {
      result = result.filter(filter.refine);
    }
    rawEntries = result;
    if (selected >= rawEntries.length) {
      selected = Math.max(0, rawEntries.length - 1);
    }
    for (const e of rawEntries) {
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
            // Rust-Log
          })
          .finally(() => thumbsInflight.delete(e.uuid));
      }
    }
  }

  async function loadTargetApp() {
    try {
      targetApp = await historyTargetApp();
      if (targetApp?.id) {
        targetIcon = await sourceAppIcon(targetApp.id);
      } else {
        targetIcon = null;
      }
    } catch {
      targetApp = null;
      targetIcon = null;
    }
  }

  onMount(() => {
    const stopTheme = initTheme();
    // Layout-Prefs
    const w = Number(localStorage.getItem(LS_LIST_W));
    if (w >= 220 && w <= 560) {
      listW = w;
    }
    showPreview = localStorage.getItem(LS_PREVIEW) !== "0";
    showMeta = localStorage.getItem(LS_META) !== "0";
    const sk = localStorage.getItem(LS_SORT) as SortKey | null;
    if (sk && ["last_copy", "first_copy", "copy_count", "size"].includes(sk)) {
      sortKey = sk;
    }
    sortReverse = localStorage.getItem(LS_SORT_REV) === "1";
    groupByDate = localStorage.getItem(LS_GROUP) === "1";

    searchInput?.focus();
    const unlistenChanged = onHistoryChanged(() => {
      if (!document.hidden) {
        refresh();
      }
    });
    const unlistenShown = listen("history-shown", () => {
      hoverAnchor = null;
      hoverSelectionEnabled = false;
      selected = 0;
      if (query === "") {
        refresh();
      } else {
        query = "";
      }
      filterId = FILTERS[0].id;
      searchInput?.focus();
      loadTargetApp();
    });
    loadTargetApp();

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
    // biome-ignore lint/suspicious/noUnusedExpressions: $effect tracking
    query;
    // biome-ignore lint/suspicious/noUnusedExpressions: $effect tracking
    filterId;
    selected = 0;
    refresh();
  });

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

  // Volles Vorschaubild lazy laden (Thumbnail überbrückt bis dahin).
  $effect(() => {
    const entry = current;
    if (!entry || entry.kind !== KIND_IMAGE) {
      fullImage = null;
      fullImageUuid = "";
      return;
    }
    if (entry.uuid === fullImageUuid) {
      return;
    }
    fullImageUuid = entry.uuid;
    const cached = fullImageCache.get(entry.uuid);
    if (cached !== undefined) {
      fullImage = cached;
      return;
    }
    fullImage = null;
    const requested = entry.uuid;
    entryImage(entry.uuid)
      .then((img) => {
        if (img) {
          if (fullImageCache.size > 40) {
            fullImageCache.clear();
          }
          fullImageCache.set(requested, img);
        }
        if (fullImageUuid === requested) {
          fullImage = img;
        }
      })
      .catch(() => {
        // Rust-Log; Thumbnail bleibt Fallback
      });
  });

  $effect(() => {
    const id = current?.source_app_id;
    if (!id || id in appIcons || appIconsInflight.has(id)) {
      return;
    }
    appIconsInflight.add(id);
    sourceAppIcon(id)
      .then((url) => {
        if (url) {
          appIcons = { ...appIcons, [id]: url };
        }
      })
      .catch(() => {
        // Rust-Log
      })
      .finally(() => appIconsInflight.delete(id));
  });

  // OCR-/QR-State zurücksetzen beim Eintragswechsel
  $effect(() => {
    const u = current?.uuid ?? "";
    if (u !== ocrUuid) {
      ocrSeq += 1;
      ocrText = null;
      ocrBlocks = [];
      ocrError = "";
      ocrBusy = false;
      ocrUuid = u;
      qrSeq += 1;
      qrResults = null;
      qrError = "";
      qrBusy = false;
    }
  });

  // Live-TOTP-Code für den ausgewählten Eintrag (aus dem vollen Text erzeugt,
  // nicht aus der ggf. gekürzten preview) und sekündlich aktualisiert.
  $effect(() => {
    const entry = current;
    const text = previewText;
    if (!(entry && isTotp(entry)) || text === null) {
      totp = null;
      return;
    }
    const cfg = parseTotp(text);
    if (!cfg) {
      totp = null;
      return;
    }
    let cancelled = false;
    const tick = () => {
      totpNow(cfg).then((next) => {
        if (!cancelled) {
          totp = next;
        }
      });
    };
    tick();
    const id = setInterval(tick, 1000);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
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

  function copyAndClose(uuid: string) {
    copyEntry(uuid)
      .then(() => hideHistoryWindow())
      .catch(() => {
        // Rust-Log; bei Fehler bleibt die Historie zur erneuten Auswahl offen.
      });
  }

  /** Aktuellen TOTP-Code des Eintrags erzeugen. Immer frisch entschlüsseln,
                  nicht aus `previewText` — das hinkt dem asynchronen Laden hinterher und
                  könnte den Code aus dem Secret des vorigen Eintrags erzeugen. */
  async function totpCode(entry: EntryDto): Promise<string | null> {
    const text = await entryText(entry.uuid);
    if (text === null) {
      return null;
    }
    const cfg = parseTotp(text);
    return cfg ? (await totpNow(cfg)).code : null;
  }

  /** Doppelklick: kopieren und schließen (bei TOTP der Code). */
  async function activateCopy(entry: EntryDto) {
    if (isTotp(entry)) {
      const code = await totpCode(entry);
      if (code) {
        await copyText(code).catch(() => {
          // Rust-Log
        });
        hideHistoryWindow();
        return;
      }
    }
    copyAndClose(entry.uuid);
  }

  /** Detail-Aktion „Kopieren" ohne Schließen (bei TOTP der Code). */
  async function copyOnly(entry: EntryDto) {
    if (isTotp(entry)) {
      const code = await totpCode(entry);
      if (code) {
        await copyText(code).catch(() => {
          // Rust-Log
        });
        return;
      }
    }
    await copyEntry(entry.uuid).catch(() => {
      // Rust-Log
    });
  }

  /** Tippen ins Zielfenster (bei TOTP der Code, sonst der Eintragsinhalt). */
  async function doType(entry: EntryDto) {
    if (isTotp(entry)) {
      const code = await totpCode(entry);
      if (code) {
        await typeText(code).catch(() => {
          // Rust-Log
        });
        return;
      }
    }
    await typeEntry(entry.uuid).catch(() => {
      // Rust-Log
    });
  }

  function doOpen(entry: EntryDto) {
    openEntry(entry.uuid).catch(() => {
      // Rust-Log
    });
  }

  /** Kontextuelle Primäraktion (SHIFT+Enter / Detail-Aktionsbutton). */
  function doAction(entry: EntryDto) {
    const action = primaryAction(entry);
    if (action === "open") {
      doOpen(entry);
    } else if (action === "extract") {
      runOcr();
    } else {
      doType(entry);
    }
  }

  /** 6-stelligen Code als „287 082" gruppieren (8-stellig: „4611 9246"). */
  function formatCode(code: string): string {
    const half = Math.ceil(code.length / 2);
    return `${code.slice(0, half)} ${code.slice(half)}`;
  }

  /** Enter tippt ins Zielfenster; ⇧+Enter nutzt die kontextuelle Primäraktion. */
  async function onEnter(entry: EntryDto, e: KeyboardEvent) {
    if (e.shiftKey) {
      doAction(entry);
    } else {
      await doType(entry);
    }
  }

  function selectFromPointer(index: number, event: MouseEvent) {
    if (hoverSelectionEnabled) {
      selected = index;
      return;
    }
    if (hoverAnchor === null) {
      hoverAnchor = { x: event.screenX, y: event.screenY };
      return;
    }
    if (event.screenX !== hoverAnchor.x || event.screenY !== hoverAnchor.y) {
      hoverSelectionEnabled = true;
      selected = index;
    }
  }

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
    const cur = entries[selected];
    if (e.key === "Escape") {
      e.preventDefault();
      if (sortOpen) {
        sortOpen = false;
        return;
      }
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
    } else if (e.key === "Enter" && cur) {
      e.preventDefault();
      await onEnter(cur, e);
    } else if (
      primaryModifierPressed(e) &&
      (e.key === "p" || e.key === "P") &&
      cur
    ) {
      e.preventDefault();
      await pinEntry(cur.uuid, !cur.pinned).catch(() => {
        // Rust-Log
      });
    } else if (
      e.key === "Delete" &&
      (primaryModifierPressed(e) || e.shiftKey) &&
      cur
    ) {
      e.preventDefault();
      await deleteEntry(cur.uuid).catch(() => {
        // Rust-Log
      });
    } else {
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

  function persistListW() {
    localStorage.setItem(LS_LIST_W, String(listW));
  }

  function togglePreview() {
    showPreview = !showPreview;
    localStorage.setItem(LS_PREVIEW, showPreview ? "1" : "0");
  }

  function toggleMeta() {
    showMeta = !showMeta;
    localStorage.setItem(LS_META, showMeta ? "1" : "0");
  }

  function restoreSelected(uuid: string | undefined) {
    if (!uuid) {
      return;
    }
    const next = entries.findIndex((entry) => entry.uuid === uuid);
    if (next >= 0) {
      selected = next;
    }
  }

  function setSort(key: SortKey) {
    const selectedUuid = current?.uuid;
    if (sortKey === key) {
      sortReverse = !sortReverse;
    } else {
      sortKey = key;
      sortReverse = false;
    }
    restoreSelected(selectedUuid);
    localStorage.setItem(LS_SORT, sortKey);
    localStorage.setItem(LS_SORT_REV, sortReverse ? "1" : "0");
    sortOpen = false;
  }

  function toggleSortReverse() {
    const selectedUuid = current?.uuid;
    sortReverse = !sortReverse;
    restoreSelected(selectedUuid);
    localStorage.setItem(LS_SORT_REV, sortReverse ? "1" : "0");
    sortOpen = false;
  }

  // ---- Resize list column ----
  function startResize(e: PointerEvent) {
    e.preventDefault();
    const startX = e.clientX;
    const startW = listW;
    const onMove = (ev: PointerEvent) => {
      listW = Math.min(560, Math.max(220, startW + (ev.clientX - startX)));
    };
    const onUp = () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      persistListW();
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  }

  async function runOcr() {
    if (!current || current.kind !== KIND_IMAGE || ocrBusy) {
      return;
    }
    const uuid = current.uuid;
    const seq = ++ocrSeq;
    ocrBusy = true;
    ocrError = "";
    ocrText = null;
    ocrBlocks = [];
    try {
      const result = await ocrEntry(uuid);
      if (seq === ocrSeq && current?.uuid === uuid) {
        ocrText = result.text;
        ocrBlocks = result.blocks;
      }
    } catch (err) {
      if (seq === ocrSeq && current?.uuid === uuid) {
        ocrError = String(err);
      }
    } finally {
      if (seq === ocrSeq && current?.uuid === uuid) {
        ocrBusy = false;
      }
    }
  }

  function copyOcr() {
    if (!ocrText) {
      return;
    }
    copyText(ocrText).catch(() => {
      // Rust-Log
    });
  }

  async function runQr() {
    if (!current || current.kind !== KIND_IMAGE || qrBusy) {
      return;
    }
    const uuid = current.uuid;
    const seq = ++qrSeq;
    qrBusy = true;
    qrError = "";
    qrResults = null;
    try {
      const results = await qrEntry(uuid);
      if (seq === qrSeq && current?.uuid === uuid) {
        qrResults = results;
      }
    } catch (e) {
      if (seq === qrSeq && current?.uuid === uuid) {
        qrError = String(e);
      }
    } finally {
      if (seq === qrSeq && current?.uuid === uuid) {
        qrBusy = false;
      }
    }
  }

  function copyQr(payload: string) {
    copyText(payload).catch(() => {
      // Rust-Log
    });
  }

  function openQr(payload: string) {
    openLink(payload).catch(() => {
      // Rust-Log
    });
  }

  const SORT_LABELS: Record<SortKey, string> = {
    last_copy: "Letzte Kopierzeit",
    first_copy: "Erste Kopierzeit",
    copy_count: "Anzahl der Kopien",
    size: "Größe",
  };

  // ---- Datumsgruppierung (optional, nur bei Zeit-Sortierung sinnvoll) ----
  const timeSorted = $derived(
    sortKey === "last_copy" || sortKey === "first_copy"
  );
  const grouping = $derived(groupByDate && timeSorted);

  function toggleGroupByDate() {
    groupByDate = !groupByDate;
    localStorage.setItem(LS_GROUP, groupByDate ? "1" : "0");
    sortOpen = false;
  }

  /** Gruppenlabel eines Eintrags; aufeinanderfolgende gleiche Label teilen einen Header. */
  function dateGroupLabel(e: EntryDto): string {
    if (e.pinned) {
      return "Angepinnt";
    }
    const ts =
      sortKey === "first_copy"
        ? (e.first_created_at ?? e.created_at)
        : e.created_at;
    const day = new Date(ts);
    day.setHours(0, 0, 0, 0);
    const today = new Date();
    today.setHours(0, 0, 0, 0);
    const diffDays = Math.round((today.getTime() - day.getTime()) / 86_400_000);
    if (diffDays <= 0) {
      return "Heute";
    }
    if (diffDays === 1) {
      return "Gestern";
    }
    if (diffDays < 7) {
      return "Letzte 7 Tage";
    }
    const d = new Date(ts);
    if (
      d.getFullYear() === today.getFullYear() &&
      d.getMonth() === today.getMonth()
    ) {
      return "Dieser Monat";
    }
    return d.toLocaleString("de-DE", { month: "long", year: "numeric" });
  }
</script>

<svelte:window onkeydown={onKeydown} />

<main style="--list-w: {listW}px" class:no-preview={!showPreview}>
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
      <div class="search-acts">
        <div class="sort-wrap">
          <button
            class="icon-btn"
            onclick={() => (sortOpen = !sortOpen)}
            title="Sortieren"
            type="button"
            class:on={sortOpen}
          >
            <Icon name="sort" size={15} />
          </button>
          {#if sortOpen}
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div class="sort-menu" onpointerdown={(e) => e.stopPropagation()}>
              {#each Object.entries(SORT_LABELS) as [k, label] (k)}
                <button
                  class="sort-item"
                  onclick={() => setSort(k as SortKey)}
                  type="button"
                  class:active={sortKey === k}
                >
                  {#if sortKey === k}
                    <span class="check">✓</span>
                  {:else}
                    <span class="check"></span>
                  {/if}
                  {label}
                </button>
              {/each}
              <div class="sort-sep"></div>
              <button
                class="sort-item"
                onclick={toggleSortReverse}
                type="button"
              >
                <span class="check">{sortReverse ? "✓" : ""}</span>
                Reihenfolge umkehren
              </button>
              <button
                class="sort-item"
                disabled={!timeSorted}
                onclick={toggleGroupByDate}
                title={timeSorted
                  ? undefined
                  : "Nur bei Sortierung nach Kopierzeit"}
                type="button"
              >
                <span class="check">{groupByDate ? "✓" : ""}</span>
                Nach Datum gruppieren
              </button>
            </div>
          {/if}
        </div>
        <button
          class="icon-btn"
          onclick={togglePreview}
          title={showPreview
            ? "Vorschaubereich ausblenden"
            : "Vorschaubereich einblenden"}
          type="button"
          class:on={!showPreview}
        >
          <Icon name="panel-preview" size={15} />
        </button>
        <button
          class="icon-btn"
          onclick={toggleMeta}
          title={showMeta ? "Details ausblenden" : "Details einblenden"}
          type="button"
          class:on={!showMeta}
        >
          <Icon name="panel-meta" size={15} />
        </button>
        <button
          aria-label="Schließen"
          class="icon-btn close"
          onclick={() => hideHistoryWindow()}
          title="Schließen (Esc)"
          type="button"
        >
          <Icon name="x" size={13} />
        </button>
      </div>
    </div>

    <section class="list" bind:this={listEl}>
      {#each entries as entry, i (entry.uuid)}
        {#if grouping && (i === 0 || dateGroupLabel(entry) !== dateGroupLabel(entries[i - 1]))}
          <div class="group-head">{dateGroupLabel(entry)}</div>
        {/if}
        <div
          aria-selected={i === selected}
          class="row"
          data-idx={i}
          ondblclick={() => activateCopy(entry)}
          onmousemove={(event) => selectFromPointer(i, event)}
          role="option"
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
        </div>
      {:else}
        <p class="empty">
          {query ? "Nichts gefunden." : "Noch nichts kopiert."}
        </p>
      {/each}
    </section>

    <footer>
      <div class="foot-left">
        <button
          class="foot-ic"
          disabled={selected <= 0}
          onclick={() => {
            selected = Math.max(0, selected - 1);
            scrollToSelected();
          }}
          title="Vorheriger"
          type="button"
        >
          <Icon name="arrow-up" size={14} />
        </button>
        <button
          class="foot-ic"
          disabled={selected >= entries.length - 1}
          onclick={() => {
            selected = Math.min(entries.length - 1, selected + 1);
            scrollToSelected();
          }}
          title="Nächster"
          type="button"
        >
          <Icon name="arrow-down" size={14} />
        </button>
      </div>
      <button
        class="foot-action"
        disabled={!current || current.kind === KIND_IMAGE}
        onclick={() => current && doType(current)}
        title="Tippen (Enter)"
        type="button"
      >
        <Icon name="return" size={13} />
        <span>
          {#if targetApp}
            In {targetApp.name} einfügen
          {:else}
            Einfügen
          {/if}
        </span>
        {#if targetIcon}
          <img alt="" class="foot-app" src={targetIcon}>
        {:else if targetApp}
          <span class="foot-app fb"><Icon name="app" size={12} /></span>
        {/if}
      </button>
    </footer>
  </div>

  {#if showPreview}
    <button
      aria-label="Spaltenbreite anpassen"
      class="splitter"
      onpointerdown={startResize}
      title="Breite ziehen"
      type="button"
    ></button>

    <aside class="detail">
      {#if current}
        <div class="detail-bar">
          <button
            class="act"
            onclick={() => copyOnly(current)}
            title={currentIsTotp ? "Code kopieren" : "Kopieren"}
            type="button"
          >
            <Icon name="copy" size={15} />
          </button>
          {#if currentAction === "open"}
            <button
              class="act"
              onclick={() => doAction(current)}
              title="Öffnen (⇧+Enter)"
              type="button"
            >
              <Icon name="external" size={15} />
            </button>
          {:else if currentAction === "extract"}
            <button
              class="act"
              disabled={ocrBusy}
              onclick={runOcr}
              title="Text extrahieren (⇧+Enter)"
              type="button"
            >
              <Icon name="scan" size={15} />
            </button>
          {:else if current.kind !== KIND_IMAGE}
            <button
              class="act"
              onclick={() => doType(current)}
              title={currentIsTotp
                ? "Code tippen (Enter)"
                : "Tippen (Enter)"}
              type="button"
            >
              <Icon name="keyboard" size={15} />
            </button>
          {/if}
          {#if current.kind === KIND_IMAGE}
            <button
              class="act"
              disabled={qrBusy}
              onclick={runQr}
              title="QR-Code lesen"
              type="button"
            >
              <Icon name="qr" size={15} />
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

        <div class="viewer">
          {#if current.kind === KIND_IMAGE}
            {#if fullImage || thumbs[current.uuid]}
              <div class="img-wrap" class:scanning={ocrBusy}>
                <!-- biome-ignore lint/a11y/noNoninteractiveElementInteractions: onload misst die gerenderte Bildbox fürs OCR-Overlay -->
                <img
                  alt="Bildvorschau"
                  onload={measureOcrBox}
                  src={fullImage ?? thumbs[current.uuid]}
                  bind:this={previewImg}
                >
                {#if ocrBlocks.length}
                  <div
                    class="ocr-overlay"
                    style="left:{ocrBox.left}px; top:{ocrBox.top}px; width:{ocrBox.width}px; height:{ocrBox.height}px;"
                  >
                    {#each ocrBlocks as block, i (i)}
                      <span
                        class="ocr-word"
                        style="left:{block.x * 100}%; top:{block.y *
                          100}%; width:{block.w * 100}%; height:{block.h *
                          100}%; font-size:{block.h * 78}cqh;"
                        >{block.text}</span
                      >
                    {/each}
                  </div>
                {/if}
                {#if ocrBusy}
                  <div class="scan-line"></div>
                {/if}
              </div>
            {:else}
              <span class="muted pad">Bild wird geladen…</span>
            {/if}
            {#if ocrText}
              <section class="block">
                <div class="block-head">
                  <span class="block-label">Extrahierter Text</span>
                  <button class="link-btn" onclick={copyOcr} type="button">
                    Kopieren
                  </button>
                </div>
                <pre class="block-body">{ocrText}</pre>
              </section>
            {:else if ocrError}
              <p class="ocr-err pad">{ocrError}</p>
            {/if}
            {#if qrResults !== null}
              <section class="block">
                <div class="block-head">
                  <span class="block-label">
                    {qrResults.length > 1 ? "QR-Codes" : "QR-Code"}
                  </span>
                </div>
                {#if qrResults.length === 0}
                  <p class="qr-empty">Kein QR-Code gefunden.</p>
                {:else}
                  {#each qrResults as payload, i (i)}
                    <div class="qr-item">
                      <pre class="block-body qr-text">{payload}</pre>
                      <div class="qr-acts">
                        <button
                          class="link-btn"
                          onclick={() => copyQr(payload)}
                          type="button"
                        >
                          Kopieren
                        </button>
                        {#if QR_LINK_RE.test(payload.trim())}
                          <button
                            class="link-btn"
                            onclick={() => openQr(payload.trim())}
                            type="button"
                          >
                            Öffnen
                          </button>
                        {/if}
                      </div>
                      {#if QR_OTPAUTH_RE.test(payload.trim())}
                        <p class="qr-hint">
                          Kopieren legt einen TOTP-Eintrag mit Live-Code an.
                        </p>
                      {/if}
                    </div>
                  {/each}
                {/if}
              </section>
            {:else if qrError}
              <p class="ocr-err pad">{qrError}</p>
            {/if}
          {:else}
            {#if currentIsTotp && totp}
              <div class="totp">
                <div class="totp-code">{formatCode(totp.code)}</div>
                <div class="totp-bar" class:low={totp.remaining <= 5}>
                  <div
                    class="totp-fill"
                    style="width:{(totp.remaining / totp.period) * 100}%"
                  ></div>
                </div>
                <div class="totp-sub">
                  <Icon name="clock" size={12} />
                  Neuer Code in {totp.remaining}s
                </div>
              </div>
            {/if}
            {#if previewText !== null}
              <pre class="text-view">{previewText}</pre>
            {:else}
              <span class="muted pad">…</span>
            {/if}
          {/if}
        </div>

        {#if showMeta}
          <div class="meta">
            <div class="block-label meta-title">Details</div>
            <div class="meta-row">
              <span class="meta-label">Anwendung</span>
              <span class="meta-value app">
                {#if current.source_app_id && appIcons[current.source_app_id]}
                  <img
                    alt=""
                    class="app-ic"
                    src={appIcons[current.source_app_id]}
                  >
                {:else}
                  <span class="app-ic fallback"
                    ><Icon name="app" size={14} /></span
                  >
                {/if}
                <span class:dim={!current.source_app_name}>
                  {current.source_app_name ?? "Unbekannt"}
                </span>
              </span>
            </div>
            <div class="meta-row">
              <span class="meta-label">Typ</span>
              <span class="meta-value">{entryMeta(current).label}</span>
            </div>
            {#if current.kind === KIND_IMAGE}
              <div class="meta-row">
                <span class="meta-label">Bildgröße</span>
                <span class="meta-value">{fmtBytes(current.size_bytes)}</span>
              </div>
            {:else}
              <div class="meta-row">
                <span class="meta-label">Größe</span>
                <span class="meta-value">{fmtBytes(current.size_bytes)}</span>
              </div>
            {/if}
            {#if currentLink}
              <div class="meta-row">
                <span class="meta-label">URL</span>
                <span class="meta-value" title={currentLink}
                  >{currentLink}</span
                >
              </div>
            {/if}
            <div class="meta-row">
              <span class="meta-label">Kopierzeit</span>
              <span class="meta-value">{fmtTime(current.created_at)}</span>
            </div>
          </div>
        {/if}
      {:else}
        <div class="viewer center">
          <span class="muted">Kein Eintrag ausgewählt</span>
        </div>
      {/if}
    </aside>
  {/if}
</main>

<style>
  :global(html),
  :global(body) {
    margin: 0;
    overflow: hidden;
    font-family: var(--font-ui);
    user-select: none;
    background: transparent !important;
  }
  main {
    display: grid;
    grid-template-columns: var(--rail-w) var(--list-w) 5px 1fr;
    height: 100vh;
    overflow: hidden;
    font-size: var(--fs-row);
    color: var(--fg-body);
    background: var(--bg-base);
    border: 1px solid var(--border-window);
    border-radius: var(--r-2xl);
    box-shadow: var(--shadow-window);
  }
  main.no-preview {
    grid-template-columns: var(--rail-w) 1fr;
  }

  .rail {
    display: flex;
    flex-direction: column;
    gap: var(--s-2);
    align-items: center;
    padding: var(--s-5) 0;
    background: var(--bg-base);
    border-right: 1px solid var(--border);
  }
  .rail-item {
    display: grid;
    place-items: center;
    width: var(--rail-item);
    height: var(--rail-item);
    color: var(--fg-dim);
    cursor: pointer;
    background: transparent;
    border: 0;
    border-radius: var(--r-md);
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
    background: var(--row-selected);
  }

  .list-col {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    background: var(--bg-base);
  }
  main:not(.no-preview) .list-col {
    border-right: 0;
  }
  main.no-preview .list-col {
    border-right: 0;
  }

  .search {
    display: flex;
    flex: none;
    gap: var(--s-3);
    align-items: center;
    height: var(--search-h);
    padding: 0 var(--s-3) 0 var(--s-5);
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
  .search-acts {
    display: flex;
    flex: none;
    gap: 2px;
    align-items: center;
  }
  .icon-btn {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    color: var(--fg-dim);
    cursor: pointer;
    background: transparent;
    border: 0;
    border-radius: var(--r-md);
  }
  .icon-btn:hover,
  .icon-btn.on {
    color: var(--fg);
    background: var(--row-hover);
  }
  .icon-btn.close:hover {
    color: var(--danger);
  }
  .sort-wrap {
    position: relative;
  }
  .sort-menu {
    position: absolute;
    top: calc(100% + 4px);
    right: 0;
    z-index: 20;
    min-width: 200px;
    padding: 6px;
    background: var(--bg-raised);
    border: 1px solid var(--border);
    border-radius: var(--r-lg);
    box-shadow: var(--shadow-overlay);
  }
  .sort-item {
    display: flex;
    gap: 8px;
    align-items: center;
    width: 100%;
    padding: 7px 10px;
    font: 450 var(--fs-control) / 1 var(--font-ui);
    color: var(--fg-body);
    text-align: left;
    cursor: pointer;
    background: transparent;
    border: 0;
    border-radius: var(--r-md);
  }
  .sort-item:hover,
  .sort-item.active {
    background: var(--row-hover);
  }
  .sort-item .check {
    width: 14px;
    color: var(--accent-text);
  }
  .sort-item:disabled {
    cursor: default;
    opacity: 0.45;
  }
  .sort-sep {
    height: 1px;
    margin: 4px 6px;
    background: var(--border);
  }

  .list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }
  /* Datumsgruppen (optional): klebende Versal-Header zwischen den Zeilen. */
  .group-head {
    position: sticky;
    top: 0;
    z-index: 1;
    padding: 8px var(--s-6) 4px;
    font: 600 var(--fs-micro) / 1 var(--font-ui);
    color: var(--fg-dim);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    background: var(--bg-base);
    border-bottom: 1px solid var(--border-soft);
  }
  /* Flache, kantige Zeilen: keine Karten, keine Radien, keine Typ-Tönung —
                   Bereichstrennung über eine Haarlinie, Selektion als deckende Neutralfläche. */
  .row {
    display: flex;
    gap: var(--s-4);
    align-items: center;
    height: var(--row-h);
    padding: 0 var(--s-6);
    cursor: default;
    border-bottom: 1px solid var(--border-soft);
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
  .mini {
    flex: 1;
    align-self: center;
    max-width: 120px;
    max-height: 26px;
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

  footer {
    display: flex;
    flex: none;
    gap: var(--s-4);
    align-items: center;
    justify-content: space-between;
    height: var(--footer-h);
    padding: 0 var(--s-4);
    border-top: 1px solid var(--border);
  }
  .foot-left {
    display: flex;
    gap: 2px;
  }
  .foot-ic,
  .foot-action {
    display: inline-flex;
    gap: 6px;
    align-items: center;
    color: var(--fg-dim);
    cursor: pointer;
    background: transparent;
    border: 0;
    border-radius: var(--r-md);
  }
  .foot-ic {
    place-items: center;
    width: 28px;
    height: 28px;
  }
  .foot-ic:hover:not(:disabled),
  .foot-action:hover:not(:disabled) {
    color: var(--fg);
    background: var(--row-hover);
  }
  .foot-ic:disabled,
  .foot-action:disabled {
    cursor: default;
    opacity: 0.35;
  }
  .foot-action {
    height: 28px;
    padding: 0 10px;
    font-size: var(--fs-micro);
  }
  .foot-app {
    width: 16px;
    height: 16px;
    object-fit: contain;
    border-radius: var(--r-xs);
  }
  .foot-app.fb {
    display: grid;
    place-items: center;
  }

  .splitter {
    z-index: 2;
    width: 5px;
    padding: 0;
    margin: 0 -2px;
    cursor: col-resize;
    background: transparent;
    border: 0;
  }
  .splitter:hover {
    background: var(--accent-soft);
  }

  .detail {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    background: var(--bg-sunken);
    border-left: 1px solid var(--border);
  }
  .detail-bar {
    display: flex;
    flex: none;
    gap: var(--s-2);
    align-items: center;
    height: var(--detail-bar-h);
    padding: 0 var(--s-5);
    border-bottom: 1px solid var(--border);
  }
  .spacer {
    flex: 1;
  }
  .act {
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    color: var(--fg-dim);
    cursor: pointer;
    background: transparent;
    border: 0;
    border-radius: var(--r-md);
    transition:
      background var(--t-fast) linear,
      color var(--t-fast) linear;
  }
  .act:hover:not(:disabled) {
    color: var(--fg);
    background: var(--row-hover);
  }
  .act:disabled {
    opacity: 0.4;
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
    overflow: auto;
  }
  .viewer.center {
    display: grid;
    place-items: center;
    padding: var(--s-7);
  }
  /* Fließtext-Vorschau: volle Breite, eigener Innenabstand (Viewer ist randlos). */
  .text-view {
    padding: var(--s-6) var(--s-7);
    margin: 0;
    font-family: var(--font-mono);
    font-size: var(--fs-control);
    line-height: 1.55;
    color: var(--fg-body);
    word-break: break-word;
    white-space: pre-wrap;
    user-select: text;
  }
  .pad {
    display: block;
    padding: var(--s-6) var(--s-7);
  }
  .img-wrap {
    position: relative;
    width: 100%;
    overflow: hidden;
  }
  .img-wrap img {
    display: block;
    width: 100%;
    height: auto;
  }
  .img-wrap.scanning img {
    filter: brightness(0.85);
  }
  /* Markierbares Text-Overlay (Live-Text-Stil): transparente, positionierte
                     Zeilen exakt über den erkannten Glyphen; cqh referenziert die Bildhöhe. */
  .ocr-overlay {
    position: absolute;
    container-type: size;
    cursor: text;
  }
  .ocr-word {
    position: absolute;
    overflow: hidden;
    color: transparent;
    white-space: nowrap;
    user-select: text;
  }
  .ocr-word::selection {
    color: transparent;
    background: var(--accent-ring);
  }
  .scan-line {
    position: absolute;
    right: 0;
    left: 0;
    height: 2px;
    background: linear-gradient(
      90deg,
      transparent,
      var(--accent-text),
      transparent
    );
    box-shadow: 0 0 12px var(--accent);
    animation: scan 1.4s linear infinite;
  }
  @keyframes scan {
    from {
      top: 0%;
    }
    to {
      top: 100%;
    }
  }
  /* Flache Bereichs-Sektion (z. B. „Extrahierter Text"): Haarlinie statt Karte,
                   kleines Versal-Label als Trennung — dieselbe Sprache wie der Detail-Bereich. */
  .block {
    display: flex;
    flex-direction: column;
    border-top: 1px solid var(--border);
  }
  .block-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: var(--s-5) var(--s-7) var(--s-2);
  }
  .block-label {
    font: 600 var(--fs-micro) / 1 var(--font-ui);
    color: var(--fg-dim);
    text-transform: uppercase;
    letter-spacing: 0.09em;
  }
  .block-body {
    padding: 0 var(--s-7) var(--s-6);
    margin: 0;
    font-family: var(--font-mono);
    font-size: var(--fs-control);
    line-height: 1.55;
    color: var(--fg-body);
    word-break: break-word;
    white-space: pre-wrap;
    user-select: text;
  }
  .link-btn {
    padding: 2px 4px;
    font-size: var(--fs-meta);
    color: var(--accent-text);
    cursor: pointer;
    background: transparent;
    border: 0;
  }
  .link-btn:hover {
    text-decoration: underline;
  }
  .ocr-err {
    font-size: var(--fs-meta);
    color: var(--danger);
  }

  /* QR-Ergebnisse: ein Block je Code, Aktionen als Link-Buttons darunter. */
  .qr-item + .qr-item {
    border-top: 1px solid var(--border-soft);
  }
  .qr-text {
    padding-bottom: var(--s-2);
  }
  .qr-acts {
    display: flex;
    gap: var(--s-4);
    padding: 0 var(--s-6) var(--s-4);
  }
  .qr-hint {
    padding: 0 var(--s-7) var(--s-5);
    margin: 0;
    font-size: var(--fs-meta);
    color: var(--fg-dim);
  }
  .qr-empty {
    padding: 0 var(--s-7) var(--s-6);
    margin: 0;
    font-size: var(--fs-control);
    color: var(--fg-dim);
  }

  /* TOTP-Hero: großer Mono-Code + dünner Ablauf-Balken (Signatur-Element). */
  .totp {
    padding: var(--s-8) var(--s-7) var(--s-7);
    border-bottom: 1px solid var(--border);
  }
  .totp-code {
    font-family: var(--font-mono);
    font-size: var(--fs-totp);
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    color: var(--fg);
    letter-spacing: 0.06em;
    user-select: text;
  }
  .totp-bar {
    height: 3px;
    margin: var(--s-5) 0 var(--s-4);
    overflow: hidden;
    background: var(--border);
  }
  .totp-fill {
    height: 100%;
    background: var(--kind-totp);
    transition: width 1s linear;
  }
  .totp-bar.low .totp-fill {
    background: var(--danger);
    transition: none;
  }
  .totp-sub {
    display: inline-flex;
    gap: var(--s-3);
    align-items: center;
    font-size: var(--fs-meta);
    color: var(--fg-dim);
  }
  .muted {
    color: var(--fg-dim);
  }

  .meta {
    flex: none;
    padding: var(--s-2) var(--meta-pad-x) var(--s-5);
    border-top: 1px solid var(--border);
  }
  .meta-title {
    padding: var(--s-5) 0 var(--s-3);
  }
  .meta-row {
    display: flex;
    gap: var(--s-7);
    align-items: center;
    justify-content: space-between;
    padding: var(--s-3) 0;
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
    text-align: right;
    white-space: nowrap;
  }
  .meta-value.app {
    display: inline-flex;
    gap: var(--s-3);
    align-items: center;
    min-width: 0;
  }
  .meta-value.app .dim {
    color: var(--fg-dim);
  }
  .app-ic {
    flex: none;
    width: 16px;
    height: 16px;
    object-fit: contain;
    border-radius: var(--r-xs);
  }
  .app-ic.fallback {
    display: grid;
    place-items: center;
    color: var(--fg-dim);
  }
</style>
