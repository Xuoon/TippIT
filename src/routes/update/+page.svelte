<script lang="ts">
  import { onMount } from "svelte";
  import {
    closeUpdateWindow,
    installUpdate,
    pendingUpdate,
    type UpdateMetadata,
    updateWindowReady,
  } from "$lib/api";
  import Icon from "$lib/icon.svelte";
  import { initTheme } from "$lib/theme";
  import "$lib/theme.css";

  let update = $state<UpdateMetadata | null>(null);
  let busy = $state(false);
  let errorMessage = $state("");

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

  onMount(() => {
    const stopTheme = initTheme();
    initializeUpdate().catch(() => undefined);
    return stopTheme;
  });

  async function install() {
    busy = true;
    errorMessage = "";
    try {
      await installUpdate();
    } catch (e) {
      errorMessage = String(e);
      busy = false;
    }
  }
</script>

<main>
  <div class="mark">
    <Icon name="download" size={16} />
  </div>
  <div class="copy">
    <strong>Neue TippIT-Version {update?.version ?? ""}</strong>
    <span>
      {busy ? "Update wird geladen und installiert…" : "Das Update ist bereit."}
    </span>
    {#if errorMessage}
      <span class="error">{errorMessage}</span>
    {/if}
    <div class="actions">
      <button
        class="btn primary"
        disabled={busy}
        onclick={install}
        type="button"
      >
        Jetzt aktualisieren
      </button>
      <button
        class="btn"
        disabled={busy}
        onclick={closeUpdateWindow}
        type="button"
      >
        Später
      </button>
    </div>
  </div>
  <button
    aria-label="Schließen"
    class="close"
    disabled={busy}
    onclick={closeUpdateWindow}
    title="Schließen"
    type="button"
  >
    <Icon name="x" size={16} />
  </button>
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
    gap: 12px;
    align-items: flex-start;
    height: 100vh;
    padding: 16px;
    font-size: var(--fs-control);
    color: var(--fg-body);
    background: var(--bg-base);
    border: 1px solid var(--border-window);
    border-radius: var(--r-2xl);
    box-shadow: var(--shadow-window);
  }
  .mark {
    display: grid;
    flex: none;
    place-items: center;
    width: 30px;
    height: 30px;
    color: var(--accent-text);
    background: var(--bg-raised);
    border-radius: 50%;
  }
  .copy {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }
  strong {
    font-size: var(--fs-label);
    font-weight: 600;
    color: var(--fg);
  }
  span {
    color: var(--fg-muted);
  }
  .error {
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--danger);
    white-space: nowrap;
  }
  .actions {
    display: flex;
    gap: 8px;
    margin-top: 8px;
  }
  .btn {
    display: inline-flex;
    gap: 6px;
    align-items: center;
    justify-content: center;
    height: 28px;
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
    width: 26px;
    height: 26px;
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
</style>
