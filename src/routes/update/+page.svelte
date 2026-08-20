<script lang="ts">
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import {
    closeUpdateWindow,
    installUpdate,
    onUpdateProgress,
    pendingUpdate,
    restartApp,
    type UpdateMetadata,
    type UpdateProgress,
    updateWindowReady,
  } from "$lib/api";
  import Icon from "$lib/icon.svelte";
  import { initTheme } from "$lib/theme";
  import "$lib/theme.css";

  let update = $state<UpdateMetadata | null>(null);
  let progress = $state<UpdateProgress | null>(null);
  let busy = $state(false);
  let errorMessage = $state("");

  const phase = $derived(progress?.phase ?? null);
  /** null = unbestimmt (Server ohne Content-Length) → laufender Balken. */
  const percent = $derived.by(() => {
    if (phase === "installing" || phase === "done") {
      return 100;
    }
    const total = progress?.total ?? 0;
    if (!(progress && total > 0)) {
      return null;
    }
    return Math.min(100, Math.round((progress.downloaded / total) * 100));
  });

  function formatMb(bytes: number): string {
    return (bytes / 1024 / 1024).toLocaleString("de-DE", {
      maximumFractionDigits: 1,
      minimumFractionDigits: 1,
    });
  }

  const status = $derived.by(() => {
    if (errorMessage) {
      return errorMessage;
    }
    if (phase === "done") {
      return "Installiert — Neustart übernimmt die neue Version.";
    }
    if (phase === "installing") {
      return "Wird installiert…";
    }
    if (phase === "downloading") {
      const total = progress?.total ?? 0;
      return total > 0
        ? `Lädt ${formatMb(progress?.downloaded ?? 0)} von ${formatMb(total)} MB`
        : "Lädt…";
    }
    return "Bereit zur Installation.";
  });

  async function initializeUpdate() {
    // Jeder Pfad muss in ready ODER close münden — sonst bliebe das unsichtbar
    // erzeugte Fenster als Zombie hängen und der Update-Hinweis ginge verloren.
    try {
      update = await pendingUpdate().catch(() => null);
      if (update) {
        await updateWindowReady();
      } else {
        await closeUpdateWindow();
      }
    } catch {
      await closeUpdateWindow().catch(() => undefined);
    }
  }

  /** ESC ist überall der Notausstieg — hier: Hinweis wegräumen. Während des
      Downloads bleibt er wirkungslos, wie auch das deaktivierte ✕. */
  function onKeydown(event: KeyboardEvent) {
    if (event.key === "Escape" && !(busy && phase !== "done")) {
      event.preventDefault();
      closeUpdateWindow().catch(() => undefined);
    }
  }

  onMount(() => {
    const stopTheme = initTheme();
    let unlisten: UnlistenFn | null = null;
    onUpdateProgress((p) => {
      progress = p;
    })
      .then((fn) => {
        unlisten = fn;
      })
      .catch(() => undefined);
    initializeUpdate().catch(() => undefined);
    return () => {
      unlisten?.();
      stopTheme();
    };
  });

  async function install() {
    busy = true;
    errorMessage = "";
    progress = null;
    try {
      await installUpdate();
    } catch (e) {
      errorMessage = String(e);
      progress = null;
      busy = false;
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

<main>
  <header>
    <div class="mark" class:done={phase === "done"}>
      <Icon name={phase === "done" ? "check" : "download"} size={15} />
    </div>
    <div class="titles">
      <strong
        >{phase === "done" ? "Update installiert" : "Update verfügbar"}</strong
      >
      <span class="versions">
        {update?.currentVersion ?? ""} <span class="arrow">→</span>
        <b>{update?.version ?? ""}</b>
      </span>
    </div>
    <button
      aria-label="Schließen"
      class="close"
      disabled={busy && phase !== "done"}
      onclick={closeUpdateWindow}
      title="Schließen"
      type="button"
    >
      <Icon name="x" size={14} />
    </button>
  </header>

  <div class="track" class:indeterminate={busy && percent === null}>
    <div class="fill" style:width="{busy ? (percent ?? 100) : 0}%"></div>
  </div>

  <div class="statusrow">
    <span class="status" class:error={Boolean(errorMessage)}>{status}</span>
    {#if percent !== null && busy}
      <span class="percent">{percent} %</span>
    {/if}
  </div>

  <div class="actions">
    {#if phase === "done"}
      <button class="btn primary" onclick={restartApp} type="button">
        Jetzt neu starten
      </button>
      <button class="btn" onclick={closeUpdateWindow} type="button">
        Später
      </button>
    {:else}
      <button
        class="btn primary"
        disabled={busy}
        onclick={install}
        type="button"
      >
        {errorMessage ? "Erneut versuchen" : "Jetzt aktualisieren"}
      </button>
      <button
        class="btn"
        disabled={busy}
        onclick={closeUpdateWindow}
        type="button"
      >
        Später
      </button>
    {/if}
  </div>
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
    display: flex;
    flex-direction: column;
    gap: 10px;
    height: 100vh;
    padding: 14px 16px;
    font-size: var(--fs-control);
    color: var(--fg-body);
    background: var(--bg-base);
    border: 1px solid var(--border-window);
    border-radius: var(--r-2xl);
    box-shadow: var(--shadow-window);
  }
  header {
    display: flex;
    gap: 10px;
    align-items: center;
  }
  .mark {
    display: grid;
    flex: none;
    place-items: center;
    width: 28px;
    height: 28px;
    color: var(--accent-text);
    background: var(--accent-soft);
    border-radius: 50%;
  }
  .mark.done {
    color: var(--success);
    background: var(--success-soft);
  }
  .titles {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  strong {
    font-size: var(--fs-label);
    font-weight: 600;
    color: var(--fg);
  }
  .versions {
    font-family: var(--font-mono);
    font-size: var(--fs-micro);
    color: var(--fg-dim);
  }
  .versions b {
    font-weight: 600;
    color: var(--fg-muted);
  }
  .arrow {
    color: var(--fg-disabled);
  }

  /* ---- Fortschritt ---- */
  .track {
    position: relative;
    height: 4px;
    overflow: hidden;
    background: var(--bg-strong);
    border-radius: var(--r-full);
  }
  .fill {
    height: 100%;
    background: var(--accent);
    border-radius: inherit;
    transition: width var(--t-base) linear;
  }
  /* Ohne Content-Length ist kein Prozentwert möglich — statt eines stehenden
     Vollbalkens läuft ein Streifen durch. */
  .indeterminate .fill {
    width: 35% !important;
    animation: slide 1.1s ease-in-out infinite;
  }
  @keyframes slide {
    0% {
      transform: translateX(-100%);
    }
    100% {
      transform: translateX(300%);
    }
  }
  .statusrow {
    display: flex;
    gap: 8px;
    align-items: baseline;
    justify-content: space-between;
  }
  .status {
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: var(--fs-meta);
    color: var(--fg-muted);
    white-space: nowrap;
  }
  .status.error {
    color: var(--danger);
  }
  .percent {
    flex: none;
    font-family: var(--font-mono);
    font-size: var(--fs-micro);
    color: var(--fg-dim);
  }

  .actions {
    display: flex;
    gap: 8px;
    margin-top: auto;
  }
  .btn {
    display: inline-flex;
    flex: 1;
    gap: 6px;
    align-items: center;
    justify-content: center;
    height: 30px;
    padding: 0 12px;
    font: 500 var(--fs-button) / 1 var(--font-ui);
    color: var(--fg-body);
    cursor: pointer;
    background: var(--bg-raised);
    border: 0;
    border-radius: var(--r-md);
    transition:
      background var(--t-fast) linear,
      color var(--t-fast) linear;
  }
  .btn:hover {
    color: var(--fg);
    background: var(--bg-hover);
  }
  .btn.primary {
    color: var(--fg-on-accent);
    background: var(--accent);
  }
  .btn.primary:hover {
    background: var(--accent-hover);
  }
  .btn:disabled {
    pointer-events: none;
    cursor: default;
    opacity: 0.45;
  }
  .close {
    display: grid;
    flex: none;
    place-items: center;
    width: 24px;
    height: 24px;
    color: var(--fg-dim);
    cursor: pointer;
    background: transparent;
    border: 0;
    border-radius: var(--r-md);
    transition:
      background var(--t-fast) linear,
      color var(--t-fast) linear;
  }
  .close:hover {
    color: var(--danger);
    background: var(--bg-hover);
  }
  .close:disabled {
    pointer-events: none;
    opacity: 0.4;
  }
</style>
