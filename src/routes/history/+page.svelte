<script lang="ts">
  import { onMount } from "svelte";
  import {
    copyEntry,
    copyText,
    createSnippet,
    deleteEntry,
    type EntryDto,
    entryHtml,
    entryImage,
    entryText,
    entryThumb,
    hideHistoryWindow,
    historyTargetApp,
    KIND_IMAGE,
    type OcrBlock,
    ocrEntry,
    onHistoryChanged,
    onHistoryShown,
    openEntry,
    openLink,
    pinEntry,
    qrEntry,
    saveEntryImage,
    searchHistory,
    setEntrySnippet,
    sourceAppIcon,
    type TargetAppDto,
    typeEntry,
    typeText,
    type WriteMode,
  } from "$lib/api";
  import HistoryList from "$lib/components/history-list.svelte";
  import Menu, { type MenuItem } from "$lib/components/menu.svelte";
  import Sheet from "$lib/components/sheet.svelte";
  import TrashView from "$lib/components/trash-view.svelte";
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
  import { fmtBytes, fmtTime, formatCode } from "$lib/format";
  import {
    detectLanguage,
    HIGHLIGHT_MAX_CHARS,
    highlight,
    LANGUAGES,
  } from "$lib/highlight";
  import { TrashState } from "$lib/history/trash.svelte";
  import Icon from "$lib/icon.svelte";
  import {
    applyWindowChrome,
    primaryModifierLabel,
    primaryModifierPressed,
  } from "$lib/platform";
  import { renderText } from "$lib/preview";
  import { SHORTCUTS } from "$lib/shortcuts";
  import { initTheme } from "$lib/theme";
  import "$lib/theme.css";
  import {
    maskOtpauthSecret,
    parseTotp,
    type TotpNow,
    totpNow,
  } from "$lib/totp";

  const LS_LIST_W = "tippit.history.listW";
  const LS_PREVIEW = "tippit.history.showPreview";
  const LS_META = "tippit.history.showMeta";
  const LS_SORT = "tippit.history.sortKey";
  const LS_SORT_REV = "tippit.history.sortRev";
  const LS_GROUP = "tippit.history.groupByDate";

  const WHITESPACE_RE = /\s+/;
  const DIGIT_KEY_RE = /^[1-9]$/;

  let query = $state("");
  let filterId = $state(FILTERS[0].id);
  let rawEntries = $state<EntryDto[]>([]);
  let selected = $state(0);
  let searchInput: HTMLInputElement | undefined = $state();
  let list: HistoryList | undefined = $state();
  const LIST_ID = "history-list";
  const TRASH_LIST_ID = "trash-list";
  let thumbs = $state<Record<string, string>>({});
  const thumbsInflight = new Set<string>();
  const THUMB_CACHE_MAX = 200;
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
  const HTTP_URL_RE = /^https?:\/\//i;
  const QR_OTPAUTH_RE = /^otpauth:\/\//i;
  // Gerenderte Bildbox (px) fürs Overlay — die object-fit:contain-Skalierung
  // ist ohne Messung nicht in CSS abbildbar, die Boxen aus Vision sind aber
  // normalisiert und mappen so exakt auf die sichtbaren Glyphen.
  let previewImg = $state<HTMLImageElement | null>(null);
  let ocrBox = $state({ left: 0, top: 0, width: 0, height: 0 });

  // TOTP: aus dem entschlüsselten Text (Secret/otpauth) live erzeugter Code.
  let totp = $state<TotpNow | null>(null);
  // Das Secret steht erst auf Wunsch im Klartext da; der Live-Code macht es
  // für den Alltag überflüssig.
  let showSecret = $state(false);

  // Formatierte Fassung (sanitisiertes HTML aus Rust) — nur für Einträge, die
  // eine haben, und nur für den ausgewählten.
  let richHtml = $state<string | null>(null);
  let richUuid = "";
  let showRich = $state(true);

  // Syntax-Hervorhebung: erkannte Sprache, im Detail-Bereich umschaltbar.
  // `langOverride` gilt nur für den gerade ausgewählten Eintrag.
  let langOverride = $state<string | null>(null);
  let langOverrideUuid = "";
  let langMenuOpen = $state(false);

  // Papierkorb ist eine eigene Ansicht, kein Filter (s. TrashState).
  let trashMode = $state(false);
  const trash = new TrashState();

  // Baustein-Verfassen (kleines Overlay).
  let composerOpen = $state(false);
  let composerText = $state("");
  let composerEl: HTMLTextAreaElement | undefined = $state();

  let cheatsheetOpen = $state(false);

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
  /** Synchron statt über `totp`: der Code kommt erst nach einem async Tick,
      bis dahin stünde das Secret kurz sichtbar da. */
  const totpConfigured = $derived(
    currentIsTotp && previewText !== null && parseTotp(previewText) !== null
  );
  const currentAction = $derived<EntryAction | null>(
    current ? primaryAction(current) : null
  );
  const currentLink = $derived(
    current && isLink(current) ? (previewText ?? current.preview) : null
  );

  /** Erkannte bzw. gewählte Sprache des ausgewählten Eintrags (null = kein Code). */
  const currentLang = $derived.by(() => {
    if (!current || current.kind === KIND_IMAGE || previewText === null) {
      return null;
    }
    if (langOverrideUuid === current.uuid && langOverride !== null) {
      return langOverride === "none" ? null : langOverride;
    }
    return detectLanguage(previewText);
  });

  /** Der Vorschautext als HTML: Code eingefärbt, sonst Links/Farben/Treffer. */
  const previewHtml = $derived.by(() => {
    if (previewText === null) {
      return "";
    }
    if (previewText.length > HIGHLIGHT_MAX_CHARS) {
      // Sehr lange Texte bleiben roh — jede Aufbereitung wäre hier spürbar.
      return null;
    }
    return currentLang
      ? highlight(previewText, currentLang)
      : renderText(previewText, query);
  });

  /** Zeichen / Zeilen / Wörter des ausgewählten Texteintrags. */
  const textStats = $derived.by(() => {
    if (previewText === null) {
      return null;
    }
    return {
      chars: previewText.length,
      lines: previewText.split("\n").length,
      words: previewText.trim()
        ? previewText.trim().split(WHITESPACE_RE).length
        : 0,
    };
  });

  let refreshSeq = 0;

  /** `preserve`: die Auswahl folgt dem Eintrag (uuid), nicht dem Index —
      Kopieren, Anpinnen oder eine neue Kopie verschieben die Reihenfolge,
      und Enter fügte sonst still einen anderen Eintrag ein. */
  async function refresh(preserve = false) {
    // Nur bei preserve lesen: refresh läuft auch synchron im Query-Effekt,
    // und jeder Lesezugriff vor dem await würde dort zur Abhängigkeit.
    const keep = preserve ? current?.uuid : undefined;
    const seq = ++refreshSeq;
    let result = await searchHistory(query, filter.backendKind);
    if (seq !== refreshSeq) {
      return;
    }
    if (filter.refine) {
      result = result.filter(filter.refine);
    }
    rawEntries = result;
    restoreSelected(keep);
    // Eintrag verschwunden (gelöscht): der Index bleibt, der Nachbar rückt nach.
    if (selected >= rawEntries.length) {
      selected = Math.max(0, rawEntries.length - 1);
    }
    if (preserve) {
      scrollToSelected();
    }
  }

  /** Thumbnail eines Bild-Eintrags nachladen, falls noch nicht vorhanden. */
  function ensureThumb(e: EntryDto) {
    if (
      e.kind !== KIND_IMAGE ||
      !e.has_thumb ||
      e.uuid in thumbs ||
      thumbsInflight.has(e.uuid)
    ) {
      return;
    }
    thumbsInflight.add(e.uuid);
    entryThumb(e.uuid)
      .then((t) => {
        if (!t) {
          return;
        }
        // Gedeckelt: bei tausenden Bildern soll nicht jedes je gesehene
        // Thumbnail im Speicher bleiben. Das älteste fällt zuerst heraus.
        const next = { ...thumbs, [e.uuid]: t };
        const keys = Object.keys(next);
        if (keys.length > THUMB_CACHE_MAX) {
          delete next[keys[0]];
        }
        thumbs = next;
      })
      .catch(() => {
        // Rust-Log
      })
      .finally(() => thumbsInflight.delete(e.uuid));
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
    applyWindowChrome();
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
        refresh(true);
      }
    });
    const unlistenShown = onHistoryShown(() => {
      hoverAnchor = null;
      hoverSelectionEnabled = false;
      trashMode = false;
      composerOpen = false;
      cheatsheetOpen = false;
      sortOpen = false;
      langMenuOpen = false;
      selected = 0;
      list?.resetScroll();
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
      // Im offenen Baustein-Editor bleibt der Fokus dort; sonst landeten die
      // nächsten Tasten in der Suche.
      if (composerOpen) {
        composerEl?.focus();
        return;
      }
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
    // Sonst zeigt die Vorschau (samt TOTP-Code) bis zum Laden den Text des
    // vorigen Eintrags.
    previewText = null;
    const requested = entry.uuid;
    entryText(entry.uuid)
      .then((t) => {
        if (t !== null) {
          if (textCache.size > 100) {
            textCache.clear();
          }
          textCache.set(requested, t);
        }
        if (previewUuid === requested) {
          previewText = t;
        }
      })
      .catch(() => {
        // Rust-Log
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
          // Vollbilder sind mehrere MB groß: nur die letzten drei behalten.
          if (fullImageCache.size >= 3) {
            const oldest = fullImageCache.keys().next().value;
            if (oldest !== undefined) {
              fullImageCache.delete(oldest);
            }
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

  // Formatierte Fassung lazy laden — nur wenn der Eintrag überhaupt eine hat.
  $effect(() => {
    const entry = current;
    if (!entry?.has_html) {
      richHtml = null;
      richUuid = "";
      return;
    }
    if (entry.uuid === richUuid) {
      return;
    }
    richUuid = entry.uuid;
    richHtml = null;
    const requested = entry.uuid;
    entryHtml(entry.uuid)
      .then((html) => {
        if (richUuid === requested) {
          richHtml = html;
        }
      })
      .catch(() => {
        // Rust-Log
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

  // OCR-/QR-State und Sprachmenü zurücksetzen beim Eintragswechsel
  $effect(() => {
    const u = current?.uuid ?? "";
    if (u !== ocrUuid) {
      langMenuOpen = false;
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
      showSecret = false;
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

  // Das Sichtfenster lädt HistoryList selbst; der ausgewählte Eintrag kann
  // außerhalb liegen und braucht sein Thumbnail für die Vorschau.
  $effect(() => {
    if (current) {
      ensureThumb(current);
    }
  });

  function scrollToSelected() {
    list?.scrollToSelected();
  }

  // ---- Papierkorb ----
  function openTrash() {
    trashMode = true;
    trash.open();
  }

  function closeTrash() {
    trashMode = false;
    refresh();
  }

  // ---- Textbausteine ----
  async function toggleSnippet(entry: EntryDto) {
    await setEntrySnippet(entry.uuid, !entry.snippet).catch(() => {
      // Rust-Log
    });
  }

  function openComposer() {
    composerText = "";
    composerOpen = true;
    queueMicrotask(() => composerEl?.focus());
  }

  async function saveComposer() {
    const text = composerText.trim();
    if (!text) {
      composerOpen = false;
      return;
    }
    await createSnippet(text).catch(() => {
      // Rust-Log
    });
    composerOpen = false;
    composerText = "";
    searchInput?.focus();
  }

  // ---- Kurzmeldung im Detailbereich ----
  let saveHint = $state("");
  let saveHintError = $state(false);
  let hintTimer: ReturnType<typeof setTimeout> | undefined;
  function flashHint(message: string, error = false) {
    saveHint = message;
    saveHintError = error;
    clearTimeout(hintTimer);
    if (message) {
      hintTimer = setTimeout(() => {
        saveHint = "";
      }, 2500);
    }
  }

  // ---- Bild speichern ----
  async function saveImage(entry: EntryDto) {
    try {
      const path = await saveEntryImage(entry.uuid);
      flashHint(path ? "Bild gespeichert" : "");
    } catch (e) {
      flashHint(String(e), true);
    }
  }

  /** Klick auf einen Link in der Vorschau (eigene `pv-link`s und Links der
      formatierten Fassung): nie in der WebView navigieren, http(s) im Browser
      öffnen. Das Öffnen selbst prüft in Rust erneut auf http(s). Als
      Svelte-Action, damit kein Klick-Handler an einem nicht-interaktiven
      Element klebt. */
  function previewLinks(node: HTMLElement) {
    const onClick = (e: MouseEvent) => {
      const link = (e.target as HTMLElement | null)?.closest("a");
      if (!link) {
        return;
      }
      e.preventDefault();
      if (e.type !== "click") {
        return;
      }
      const url = link.getAttribute("data-url") ?? link.getAttribute("href");
      if (url && HTTP_URL_RE.test(url)) {
        openLink(url).catch(() => {
          // Rust-Log
        });
      }
    };
    node.addEventListener("click", onClick);
    // Auch ein Mittelklick darf keinen Link in der WebView öffnen.
    node.addEventListener("auxclick", onClick);
    return {
      destroy: () => {
        node.removeEventListener("click", onClick);
        node.removeEventListener("auxclick", onClick);
      },
    };
  }

  /** Leermeldung nach Filter — „Noch nichts kopiert" stimmt nur für „Alle". */
  const emptyMessage = $derived.by(() => {
    if (query) {
      return "Nichts gefunden.";
    }
    if (filterId === "snippets") {
      return "Noch keine Textbausteine.";
    }
    if (filterId === FILTERS[0].id) {
      return "Noch nichts kopiert.";
    }
    return `Nichts in „${filter.label}".`;
  });

  /** Klick außerhalb schließt offene Menüs. Der eigene Umschaltknopf zählt als
      innen, sonst schlösse pointerdown das Menü und der Klick öffnete es wieder. */
  function closeMenusOutside(e: PointerEvent) {
    const target = e.target as Element | null;
    if (sortOpen && !target?.closest(".sort-wrap")) {
      sortOpen = false;
    }
    if (langMenuOpen && !target?.closest(".lang-wrap")) {
      langMenuOpen = false;
    }
  }

  function setLanguage(id: string) {
    langOverride = id;
    langOverrideUuid = current?.uuid ?? "";
    langMenuOpen = false;
  }

  function cycleFilter(dir: 1 | -1) {
    const idx = FILTERS.findIndex((f) => f.id === filterId);
    filterId = FILTERS[(idx + dir + FILTERS.length) % FILTERS.length].id;
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

  /** Enter/Doppelklick: in die Zwischenablage legen UND ins zuvor aktive Feld
      schreiben (bei TOTP der Code), standardmäßig per Einfügen (STRG+V/⌘V,
      alles auf einmal); `per_char` tippt zeichenweise.
      Bilder werden immer eingefügt. Kein `hideHistoryWindow` auf den
      Schreib-Pfaden: `spawn_type` versteckt selbst und stellt vorher das
      gemerkte Zielfenster wieder her. */
  async function insertEntry(entry: EntryDto, mode: WriteMode = "paste") {
    if (isTotp(entry)) {
      const code = await totpCode(entry);
      if (code) {
        try {
          await copyText(code);
        } catch {
          // Rust-Log; ohne Code in der Zwischenablage fügte STRG+V den alten Inhalt ein.
          return;
        }
        await typeText(code, mode).catch(() => {
          // Rust-Log
        });
        return;
      }
      // Kein Code erzeugbar: hier NICHT auf den Rohtext zurückfallen — der
      // trägt bei einer otpauth-Adresse das Secret, und das darf nie ins
      // Zielfenster wandern.
      flashHint("Kein TOTP-Code erzeugbar — Eintrag prüfen.", true);
      return;
    }
    try {
      await copyEntry(entry.uuid);
    } catch {
      // Rust-Log; bei Fehler bleibt die Historie zur erneuten Auswahl offen.
      return;
    }
    const inject = entry.kind === KIND_IMAGE ? "paste" : mode;
    await typeEntry(entry.uuid, inject).catch(() => {
      // Rust-Log
    });
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
      // Ohne Code kein Rückfall auf den Rohtext (enthielte das Secret).
      flashHint("Kein TOTP-Code erzeugbar — Eintrag prüfen.", true);
      return;
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
      // Ohne Code kein Rückfall auf den Rohtext (enthielte das Secret).
      flashHint("Kein TOTP-Code erzeugbar — Eintrag prüfen.", true);
      return;
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

  /** Enter fügt ins Zielfenster ein, Strg/⌘+Enter tippt, ⇧+Enter nutzt die
      kontextuelle Primäraktion. */
  async function onEnter(entry: EntryDto, e: KeyboardEvent) {
    if (e.shiftKey) {
      doAction(entry);
    } else if (primaryModifierPressed(e)) {
      await doType(entry);
    } else {
      await insertEntry(entry);
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

  /** Esc schließt der Reihe nach: Overlays, Menüs, Papierkorb, Fenster. */
  function handleEscape(): void {
    if (composerOpen) {
      composerOpen = false;
    } else if (cheatsheetOpen) {
      cheatsheetOpen = false;
    } else if (langMenuOpen) {
      langMenuOpen = false;
    } else if (sortOpen) {
      sortOpen = false;
    } else if (trashMode) {
      closeTrash();
    } else {
      hideHistoryWindow().catch(() => {
        // Rust-Log
      });
    }
  }

  /** Navigation und Filterwechsel. `true` = Taste war zuständig. */
  function handleNavKeys(e: KeyboardEvent): boolean {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      selected = Math.min(selected + 1, entries.length - 1);
      scrollToSelected();
      return true;
    }
    if (e.key === "ArrowUp") {
      e.preventDefault();
      selected = Math.max(selected - 1, 0);
      scrollToSelected();
      return true;
    }
    if (e.key === "Tab") {
      e.preventDefault();
      cycleFilter(e.shiftKey ? -1 : 1);
      return true;
    }
    return false;
  }

  /** Aktionen auf dem ausgewählten Eintrag. `true` = Taste war zuständig. */
  async function handleEntryKeys(
    e: KeyboardEvent,
    cur: EntryDto
  ): Promise<boolean> {
    const mod = primaryModifierPressed(e);
    if (e.key === "Enter") {
      e.preventDefault();
      await onEnter(cur, e);
      return true;
    }
    if (mod && (e.key === "p" || e.key === "P")) {
      e.preventDefault();
      await pinEntry(cur.uuid, !cur.pinned).catch(() => {
        // Rust-Log
      });
      return true;
    }
    if (mod && (e.key === "b" || e.key === "B")) {
      e.preventDefault();
      await toggleSnippet(cur);
      return true;
    }
    if (e.key === "Delete" && (mod || e.shiftKey)) {
      e.preventDefault();
      await deleteEntry(cur.uuid).catch(() => {
        // Rust-Log
      });
      return true;
    }
    return false;
  }

  async function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      handleEscape();
      return;
    }
    if (composerOpen) {
      // Im Verfassen-Overlay gehören alle Tasten dem Textfeld; nur Speichern
      // per Strg/⌘+Enter wird abgefangen.
      if (e.key === "Enter" && primaryModifierPressed(e)) {
        e.preventDefault();
        await saveComposer();
      }
      return;
    }
    // Das Suchfeld hat immer den Fokus, deshalb reicht „nicht im Feld" nicht:
    // bei leerer Suche gehört „?" der Übersicht, danach dem Suchtext.
    if (
      e.key === "?" &&
      (query === "" || document.activeElement !== searchInput)
    ) {
      e.preventDefault();
      cheatsheetOpen = !cheatsheetOpen;
      return;
    }
    // Die Übersicht liegt über der Liste: Enter & Co. dürfen nicht dahinter
    // einfügen.
    if (cheatsheetOpen) {
      return;
    }
    if (trashMode) {
      await trash.handleKey(e);
      return;
    }
    // Strg/⌘+1…9: n-ten Eintrag direkt einfügen, ohne Navigieren.
    if (primaryModifierPressed(e) && DIGIT_KEY_RE.test(e.key)) {
      const target = entries[Number(e.key) - 1];
      if (target) {
        e.preventDefault();
        selected = Number(e.key) - 1;
        await insertEntry(target);
        return;
      }
    }
    if (handleNavKeys(e)) {
      return;
    }
    const cur = entries[selected];
    if (cur && (await handleEntryKeys(e, cur))) {
      return;
    }
    handleTypeToSearch(e);
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
    // capture: der Monitor soll den Inhalt erfassen (otpauth → TOTP-Eintrag).
    copyText(payload, true).catch(() => {
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

  const sortSections = $derived<MenuItem[][]>([
    (Object.keys(SORT_LABELS) as SortKey[]).map((k) => ({
      label: SORT_LABELS[k],
      checked: sortKey === k,
      role: "radio",
      onselect: () => setSort(k),
    })),
    [
      {
        label: "Reihenfolge umkehren",
        checked: sortReverse,
        role: "check",
        onselect: toggleSortReverse,
      },
      {
        label: "Nach Datum gruppieren",
        checked: groupByDate,
        role: "check",
        disabled: !timeSorted,
        title: timeSorted ? undefined : "Nur bei Sortierung nach Kopierzeit",
        onselect: toggleGroupByDate,
      },
    ],
  ]);

  const langSections = $derived<MenuItem[][]>([
    LANGUAGES.map((lang) => ({
      label: lang.label,
      checked: currentLang === lang.id,
      role: "radio",
      onselect: () => setLanguage(lang.id),
    })),
    [
      {
        label: "Ohne Hervorhebung",
        checked: currentLang === null,
        role: "radio",
        onselect: () => setLanguage("none"),
      },
    ],
  ]);

  /** Aktive Zeile fürs Suchfeld (combobox): Liste oder Papierkorb. */
  const activeDescendant = $derived.by(() => {
    if (trashMode) {
      return trash.current ? `t-${trash.current.uuid}` : undefined;
    }
    return current ? `e-${current.uuid}` : undefined;
  });
</script>

<svelte:window onkeydown={onKeydown} onpointerdown={closeMenusOutside} />

<main style="--list-w: {listW}px" class:no-preview={!showPreview}>
  <aside class="rail">
    {#each FILTERS as item (item.id)}
      <button
        aria-label={item.label}
        aria-pressed={filterId === item.id && !trashMode}
        class="rail-item"
        onclick={() => {
          trashMode = false;
          filterId = item.id;
        }}
        title={item.label}
        type="button"
        class:active={filterId === item.id && !trashMode}
      >
        <Icon name={item.icon} size={17} />
      </button>
    {/each}
    <span class="rail-spacer"></span>
    <button
      aria-label="Papierkorb"
      aria-pressed={trashMode}
      class="rail-item"
      onclick={() => (trashMode ? closeTrash() : openTrash())}
      title="Papierkorb"
      type="button"
      class:active={trashMode}
    >
      <Icon name="trash" size={17} />
    </button>
    <button
      aria-label="Tastenkürzel"
      aria-pressed={cheatsheetOpen}
      class="rail-item"
      onclick={() => (cheatsheetOpen = !cheatsheetOpen)}
      title="Tastenkürzel (?)"
      type="button"
      class:active={cheatsheetOpen}
    >
      <Icon name="help" size={17} />
    </button>
  </aside>

  <div class="list-col">
    <div class="search">
      <Icon name="search" size={15} />
      <input
        aria-activedescendant={activeDescendant}
        aria-autocomplete="list"
        aria-controls={trashMode ? TRASH_LIST_ID : LIST_ID}
        aria-expanded="true"
        aria-label="Suche"
        placeholder="Tippen Sie zum Suchen…"
        role="combobox"
        spellcheck="false"
        type="text"
        bind:this={searchInput}
        bind:value={query}
      >
      <div class="search-acts">
        {#if filterId === "snippets" && !trashMode}
          <button
            aria-label="Neuen Textbaustein anlegen"
            class="icon-btn"
            onclick={openComposer}
            title="Neuen Textbaustein anlegen"
            type="button"
          >
            <Icon name="bookmark" size={15} />
          </button>
        {/if}
        <div class="sort-wrap">
          <button
            aria-expanded={sortOpen}
            aria-haspopup="menu"
            aria-label="Sortieren"
            class="icon-btn"
            onclick={() => (sortOpen = !sortOpen)}
            title="Sortieren"
            type="button"
            class:on={sortOpen}
          >
            <Icon name="sort" size={15} />
          </button>
          {#if sortOpen}
            <Menu label="Sortieren" sections={sortSections} />
          {/if}
        </div>
        <button
          aria-label="Vorschaubereich"
          aria-pressed={showPreview}
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
          aria-label="Details"
          aria-pressed={showMeta}
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

    {#if trashMode}
      <TrashView listId={TRASH_LIST_ID} {trash} />
    {:else}
      <HistoryList
        {emptyMessage}
        {entries}
        {grouping}
        listId={LIST_ID}
        onneedthumb={ensureThumb}
        onpick={(entry, event) =>
          insertEntry(entry, primaryModifierPressed(event) ? "per_char" : "paste")}
        onpointerselect={selectFromPointer}
        {query}
        {selected}
        {sortKey}
        {thumbs}
        bind:this={list}
      />
    {/if}

    <footer>
      <div class="foot-left">
        <button
          aria-label="Vorheriger"
          class="foot-ic"
          disabled={trashMode || selected <= 0}
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
          aria-label="Nächster"
          class="foot-ic"
          disabled={trashMode || selected >= entries.length - 1}
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
        disabled={trashMode || !current}
        onclick={() => current && insertEntry(current)}
        title="Einfügen (Enter)"
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
      {#if trashMode}
        {#if trash.current}
          {@const item = trash.current}
          <div class="detail-bar">
            <button
              aria-label="Wiederherstellen"
              class="act"
              onclick={() => trash.restore(item.uuid)}
              title="Wiederherstellen (Enter)"
              type="button"
            >
              <Icon name="restore" size={15} />
            </button>
            <span class="spacer"></span>
            <button
              aria-label="Endgültig löschen"
              class="act danger"
              onclick={() => trash.purge(item.uuid)}
              title="Endgültig löschen"
              type="button"
            >
              <Icon name="trash" size={15} />
            </button>
          </div>
          <div class="viewer">
            <pre class="text-view">{maskOtpauthSecret(item.preview)}</pre>
          </div>
          <div class="meta">
            <div class="block-label meta-title">Details</div>
            <div class="meta-row">
              <span class="meta-label">Gelöscht</span>
              <span class="meta-value">{fmtTime(item.trashed_at)}</span>
            </div>
            <div class="meta-row">
              <span class="meta-label">Größe</span>
              <span class="meta-value">{fmtBytes(item.size_bytes)}</span>
            </div>
          </div>
        {:else}
          <div class="viewer center">
            <span class="muted">Der Papierkorb ist leer</span>
          </div>
        {/if}
      {:else if current}
        <div class="detail-bar">
          <button
            aria-label={currentIsTotp ? "Code kopieren" : "Kopieren"}
            class="act"
            onclick={() => copyOnly(current)}
            title={currentIsTotp ? "Code kopieren" : "Kopieren"}
            type="button"
          >
            <Icon name="copy" size={15} />
          </button>
          {#if currentAction === "open"}
            <button
              aria-label="Öffnen"
              class="act"
              onclick={() => doAction(current)}
              title="Öffnen (⇧+Enter)"
              type="button"
            >
              <Icon name="external" size={15} />
            </button>
          {:else if currentAction === "extract"}
            <button
              aria-label="Text extrahieren"
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
              aria-label={currentIsTotp ? "Code tippen" : "Tippen"}
              class="act"
              onclick={() => doType(current)}
              title={currentIsTotp
                ? `Code tippen (${primaryModifierLabel}+Enter)`
                : `Tippen (${primaryModifierLabel}+Enter)`}
              type="button"
            >
              <Icon name="keyboard" size={15} />
            </button>
          {/if}
          {#if current.kind === KIND_IMAGE}
            <button
              aria-label="QR-Code lesen"
              class="act"
              disabled={qrBusy}
              onclick={runQr}
              title="QR-Code lesen"
              type="button"
            >
              <Icon name="qr" size={15} />
            </button>
            <button
              aria-label="Als PNG speichern"
              class="act"
              onclick={() => saveImage(current)}
              title="Als PNG speichern"
              type="button"
            >
              <Icon name="save" size={15} />
            </button>
          {/if}
          {#if current.kind !== KIND_IMAGE && previewText !== null && previewText.length <= HIGHLIGHT_MAX_CHARS}
            <div class="lang-wrap">
              <button
                aria-expanded={langMenuOpen}
                aria-haspopup="menu"
                aria-label="Sprache der Hervorhebung"
                class="act"
                onclick={() => (langMenuOpen = !langMenuOpen)}
                title="Sprache der Hervorhebung"
                type="button"
                class:on={langMenuOpen}
              >
                <Icon name="code" size={15} />
              </button>
              {#if langMenuOpen}
                <Menu
                  align="left"
                  label="Sprache der Hervorhebung"
                  sections={langSections}
                />
              {/if}
            </div>
          {/if}
          {#if richHtml}
            <button
              aria-label="Formatierung"
              aria-pressed={showRich}
              class="act"
              onclick={() => (showRich = !showRich)}
              title={showRich
                ? "Als Klartext anzeigen"
                : "Formatierung anzeigen"}
              type="button"
              class:on={showRich}
            >
              <Icon name="text" size={15} />
            </button>
          {/if}
          <span class="spacer"></span>
          <button
            aria-label="Textbaustein"
            aria-pressed={current.snippet}
            class="act"
            onclick={() => toggleSnippet(current)}
            title={current.snippet
              ? `Baustein aufheben (${primaryModifierLabel}+B)`
              : `Als Textbaustein merken (${primaryModifierLabel}+B)`}
            type="button"
            class:pinned={current.snippet}
          >
            <Icon name="bookmark" size={15} />
          </button>
          <button
            aria-label="Angepinnt"
            aria-pressed={current.pinned}
            class="act"
            onclick={() =>
              pinEntry(current.uuid, !current.pinned).catch(() => {
                // Rust-Log
              })}
            title={current.pinned
              ? `Pin lösen (${primaryModifierLabel}+P)`
              : `Anpinnen (${primaryModifierLabel}+P)`}
            type="button"
            class:pinned={current.pinned}
          >
            <Icon name={current.pinned ? "star-filled" : "star"} size={15} />
          </button>
          <button
            aria-label="Löschen"
            class="act danger"
            onclick={() =>
              deleteEntry(current.uuid).catch(() => {
                // Rust-Log
              })}
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
                        {#if HTTP_URL_RE.test(payload.trim())}
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
                <div class="totp-foot">
                  <span class="totp-sub">
                    <Icon name="clock" size={12} />
                    Neuer Code in {totp.remaining}s
                  </span>
                  <button
                    class="link-btn"
                    onclick={() => (showSecret = !showSecret)}
                    type="button"
                  >
                    {showSecret ? "Secret verbergen" : "Secret anzeigen"}
                  </button>
                </div>
              </div>
            {/if}
            {#if previewText === null}
              <span class="muted pad">…</span>
            {:else if totpConfigured && !showSecret}
              <!-- Secret verborgen; ohne erzeugbaren Code (Fehlerkennung)
                   bleibt der Text dagegen sichtbar. -->
            {:else if richHtml && showRich}
              <!-- In Rust sanitisiert (ammonia) und beim Ausliefern erneut
                   gereinigt — hier kommt nie ungefiltertes Fremd-HTML an. -->
              <div class="rich-view" use:previewLinks>{@html richHtml}</div>
            {:else if previewHtml === null}
              <pre class="text-view">{previewText}</pre>
            {:else}
              <pre
                class="text-view"
                class:code={!!currentLang}
                use:previewLinks
              >{@html previewHtml}</pre>
            {/if}
          {/if}
          {#if saveHint}
            <p class="save-hint pad" role="status" class:error={saveHintError}>
              {saveHint}
            </p>
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
            {#if textStats}
              <div class="meta-row">
                <span class="meta-label">Umfang</span>
                <span class="meta-value">
                  {textStats.chars.toLocaleString("de-DE")}
                  Zeichen ·
                  {textStats.words.toLocaleString("de-DE")}
                  Wörter ·
                  {textStats.lines.toLocaleString("de-DE")}
                  Zeilen
                </span>
              </div>
            {/if}
            {#if currentLang}
              <div class="meta-row">
                <span class="meta-label">Sprache</span>
                <span class="meta-value">
                  {LANGUAGES.find((l) => l.id === currentLang)?.label ??
                    currentLang}
                </span>
              </div>
            {/if}
            {#if current.snippet}
              <div class="meta-row">
                <span class="meta-label">Platzhalter</span>
                <span class="meta-value dim">
                  {"{datum}"}
                  · {"{uhrzeit}"} · {"{datumzeit}"}
                </span>
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

  {#if composerOpen}
    <Sheet title="Neuer Textbaustein">
      <textarea
        aria-label="Text des Bausteins"
        class="composer-text"
        placeholder="Text des Bausteins — Platzhalter: {'{datum}'}, {'{uhrzeit}'}, {'{datumzeit}'}"
        rows="7"
        bind:this={composerEl}
        bind:value={composerText}
      ></textarea>
      {#snippet actions()}
        <button
          class="link-btn"
          onclick={() => (composerOpen = false)}
          type="button"
        >
          Abbrechen
        </button>
        <button class="link-btn" onclick={saveComposer} type="button">
          Anlegen ({primaryModifierLabel}+Enter)
        </button>
      {/snippet}
    </Sheet>
  {/if}

  {#if cheatsheetOpen}
    <Sheet title="Tastenkürzel">
      <dl class="keys">
        {#each SHORTCUTS as shortcut (shortcut.keys)}
          <dt><kbd>{shortcut.keys}</kbd></dt>
          <dd>{shortcut.label}</dd>
        {/each}
      </dl>
      {#snippet actions()}
        <button
          class="link-btn"
          onclick={() => (cheatsheetOpen = false)}
          type="button"
        >
          Schließen
        </button>
      {/snippet}
    </Sheet>
  {/if}
</main>

<style>
  :global(html),
  :global(body) {
    margin: 0;
    overflow: hidden;
    font-family: var(--font-ui);
    user-select: none;
  }
  /* macOS: transparentes Fenster, Rahmen und Radius zeichnet die Seite selbst.
     Windows: opakes Fenster, Ecken und Schatten kommen von DWM. */
  :global(html[data-chrome="floating"]),
  :global(html[data-chrome="floating"] body) {
    background: transparent !important;
  }
  main {
    /* Bezugsrahmen der Overlays: sonst deckt der Scrim auch die transparenten
       Ecken außerhalb des Radius ab. */
    position: relative;
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
  :global(html[data-chrome="native"]) main {
    border: 0;
    border-radius: 0;
    box-shadow: none;
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

  .totp-foot {
    display: flex;
    gap: var(--s-4);
    align-items: center;
    justify-content: space-between;
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
  .rail-spacer {
    flex: 1;
  }

  /* ---- Vorschau: Code, Links, Farbproben ---- */
  .text-view :global(mark) {
    color: var(--fg-body);
    background: var(--mark);
    border-radius: var(--r-2xs);
  }
  .text-view :global(.pv-link) {
    color: var(--accent-text);
    text-decoration: underline;
    text-underline-offset: 2px;
    cursor: pointer;
  }
  .text-view :global(.pv-color) {
    display: inline-flex;
    gap: 4px;
    align-items: center;
  }
  .text-view :global(.pv-swatch) {
    display: inline-block;
    width: 10px;
    height: 10px;
    border: 1px solid var(--border);
    border-radius: var(--r-2xs);
  }
  .text-view.code :global(.tok-keyword) {
    color: var(--tok-keyword);
  }
  .text-view.code :global(.tok-string) {
    color: var(--tok-string);
  }
  .text-view.code :global(.tok-number) {
    color: var(--tok-number);
  }
  .text-view.code :global(.tok-comment) {
    font-style: italic;
    color: var(--tok-comment);
  }
  .text-view.code :global(.tok-fn) {
    color: var(--tok-fn);
  }
  .text-view.code :global(.tok-tag) {
    color: var(--tok-tag);
  }
  .text-view.code :global(.tok-tag-name) {
    color: var(--tok-tag-name);
  }
  .text-view.code :global(.tok-attr) {
    color: var(--tok-attr);
  }

  /* ---- Formatierte Fassung ---- */
  .rich-view {
    padding: var(--s-6) var(--s-7);
    font-size: var(--fs-control);
    line-height: 1.55;
    color: var(--fg-body);
    word-break: break-word;
    user-select: text;
  }
  .rich-view :global(*) {
    max-width: 100%;
  }
  .rich-view :global(table) {
    border-collapse: collapse;
  }
  .rich-view :global(td),
  .rich-view :global(th) {
    padding: 2px 6px;
    border: 1px solid var(--border-soft);
  }

  .save-hint {
    font-size: var(--fs-micro);
    color: var(--success);
  }
  .save-hint.error {
    color: var(--danger);
  }

  .lang-wrap {
    position: relative;
  }

  /* ---- Overlays (Rahmen in sheet.svelte) ---- */
  .composer-text {
    padding: var(--s-3) var(--s-4);
    font-family: var(--font-mono);
    font-size: var(--fs-control);
    color: var(--fg-body);
    resize: vertical;
    user-select: text;
    background: var(--bg-base);
    border: 1px solid var(--border);
    border-radius: var(--r-md);
  }
  .composer-text:focus {
    outline: none;
    box-shadow: var(--shadow-focus);
  }
  .keys {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: var(--s-2) var(--s-5);
    margin: 0;
    font-size: var(--fs-control);
  }
  .keys dt {
    text-align: right;
  }
  .keys dd {
    margin: 0;
    color: var(--fg-dim);
  }
  .keys kbd {
    padding: 1px 6px;
    font: 500 var(--fs-micro) / 1.6 var(--font-ui);
    color: var(--fg-body);
    background: var(--bg-base);
    border: 1px solid var(--border);
    border-radius: var(--r-sm);
  }
</style>
