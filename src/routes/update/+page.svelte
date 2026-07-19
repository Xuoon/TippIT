<script lang="ts">
  import { onMount } from "svelte";
  import {
    closeUpdateWindow,
    installUpdate,
    pendingUpdate,
    type UpdateMetadata,
    updateWindowReady,
  } from "$lib/api";

  let update = $state<UpdateMetadata | null>(null);
  let busy = $state(false);
  let errorMessage = $state("");

  onMount(async () => {
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
  <div class="mark">↻</div>
  <div class="copy">
    <strong>Neue TippIT-Version {update?.version ?? ""}</strong>
    <span
      >{busy ? "Update wird geladen und installiert…" : "Das Update ist bereit."}</span
    >
    {#if errorMessage}
      <span class="error">{errorMessage}</span>
    {/if}
    <div class="actions">
      <button class="install" disabled={busy} onclick={install} type="button">
        Jetzt aktualisieren
      </button>
      <button disabled={busy} onclick={closeUpdateWindow} type="button">
        Später
      </button>
    </div>
  </div>
  <button
    class="close"
    disabled={busy}
    onclick={closeUpdateWindow}
    title="Schließen"
    type="button"
  >
    ×
  </button>
</main>

<style>
  :global(body) {
    margin: 0;
    overflow: hidden;
  }
  main {
    box-sizing: border-box;
    display: flex;
    gap: 12px;
    align-items: flex-start;
    height: 100vh;
    padding: 16px;
    font:
      13px "Segoe UI",
      system-ui,
      sans-serif;
    color: #d8dfef;
    background: #151821;
    border: 1px solid #343948;
  }
  .mark {
    display: grid;
    flex: none;
    place-items: center;
    width: 30px;
    height: 30px;
    color: #8eb6f5;
    background: #202a3a;
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
    font-size: 14px;
  }
  span {
    color: #929bb0;
  }
  .error {
    overflow: hidden;
    text-overflow: ellipsis;
    color: #e99aaa;
    white-space: nowrap;
  }
  .actions {
    display: flex;
    gap: 14px;
    margin-top: 8px;
  }
  button {
    padding: 0;
    color: #9da7bc;
    cursor: pointer;
    background: none;
    border: 0;
  }
  button:hover {
    color: #d8dfef;
  }
  .install {
    font-weight: 600;
    color: #8eb6f5;
  }
  button:disabled {
    cursor: default;
    opacity: 0.55;
  }
  .close {
    flex: none;
    font-size: 20px;
    line-height: 1;
    color: #737c90;
  }
</style>
