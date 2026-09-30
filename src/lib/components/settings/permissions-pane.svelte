<script lang="ts">
  import { onMount } from "svelte";
  import {
    copyText,
    openPermissionSettings,
    type PermissionStatus,
    permissionStatus,
    requestInputPermission,
    resetInputPermission,
  } from "$lib/api";
  import Icon from "$lib/icon.svelte";

  interface Props {
    /** Aktive Suche; klappt die Fehlerbehebung auf. */
    q?: string;
    /** Zeile sichtbar (Suchtreffer oder keine Suche)? */
    show: (key: string) => boolean;
  }
  const { show, q = "" }: Props = $props();

  let status = $state<PermissionStatus | null>(null);
  let busy = $state(false);
  let repairError = $state("");
  let copied = $state(false);

  const LOCATION_TEXT: Record<
    Exclude<PermissionStatus["location"], "applications">,
    string
  > = {
    translocated: "macOS startet es aus einem Zwischenordner, Updates und Autostart gehen so nicht.",
    dmg: "Es läuft direkt aus dem DMG, Updates und Autostart gehen so nicht.",
    downloads: "Es läuft aus „Downloads“, Updates und Autostart sind so unzuverlässig.",
    other: "Updates und Autostart sind so unzuverlässig.",
  };

  const repairCommand = $derived(
    `tccutil reset Accessibility ${status?.bundle_id ?? ""}`
  );

  // Die Abfrage startet codesign; überlappende Aufrufe aus Intervall und
  // Fokus-Events sparen.
  let loading = false;
  async function refresh() {
    if (loading) {
      return;
    }
    loading = true;
    try {
      status = await permissionStatus();
    } catch {
      // Ohne Status bleibt die letzte Anzeige stehen.
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    refresh();
    // Freigegeben wird in den Systemeinstellungen; beim Zurückkehren neu prüfen.
    const onVisible = () => {
      if (!document.hidden) {
        refresh();
      }
    };
    window.addEventListener("focus", refresh);
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      window.removeEventListener("focus", refresh);
      document.removeEventListener("visibilitychange", onVisible);
    };
  });

  // Die Systemeinstellungen liegen oft neben dem Fenster, ohne dass es den
  // Fokus verliert und zurückbekommt; solange die Freigabe fehlt, nachfragen.
  const untrusted = $derived(status !== null && !status.input_trusted);
  $effect(() => {
    if (!untrusted) {
      return;
    }
    const timer = setInterval(refresh, 1500);
    return () => clearInterval(timer);
  });

  async function run(action: () => Promise<unknown>) {
    busy = true;
    try {
      await action();
    } finally {
      busy = false;
      await refresh();
    }
  }

  const request = () => run(requestInputPermission);
  const openSettings = () => run(openPermissionSettings);
  const repair = () =>
    run(async () => {
      repairError = "";
      try {
        await resetInputPermission();
      } catch (e) {
        repairError = String(e);
      }
    });

  async function copy(text: string) {
    await copyText(text).catch(() => undefined);
    copied = true;
    setTimeout(() => (copied = false), 1500);
  }

  const CLIPBOARD_ACCESS: Record<string, string> = {
    allow: "erlaubt",
    ask: "fragt nach",
    default: "Systemstandard",
    deny: "verweigert",
  };

  const diagnosis = $derived(
    status
      ? [
          `Signatur: ${status.signature || "unbekannt"}`,
          `Bundle-ID: ${status.bundle_id || "unbekannt"}`,
          `Pfad: ${status.bundle_path || "unbekannt"}`,
          ...(status.clipboard_access
            ? [`Zwischenablage: ${CLIPBOARD_ACCESS[status.clipboard_access]}`]
            : []),
        ].join("\n")
      : ""
  );
</script>

{#if status?.supported}
  {#if show("permAx")}
    <div class="card">
      <div class="row">
        <span class="row-label">
          Bedienungshilfen
          <span class="row-hint">Nötig, damit TippIT tippen und einfügen kann</span>
        </span>
        {#if status.input_trusted}
          <span class="state ok"><Icon name="check" size={13} />Erteilt</span>
        {:else}
          <button
            class="btn primary"
            disabled={busy}
            onclick={request}
            type="button"
          >
            Freigeben
          </button>
        {/if}
      </div>
      {#if status.clipboard_access === "deny"}
        <div class="row">
          <span class="row-label">
            Zwischenablage
            <span class="row-hint">Verweigert, die Historie bleibt leer</span>
          </span>
          <button class="btn" disabled={busy} onclick={openSettings} type="button">
            <Icon name="external" size={14} />Öffnen
          </button>
        </div>
      {/if}
    </div>
  {/if}

  {#if show("permLocation") && status.location !== "applications"}
    <div class="notice">
      <Icon name="alert" size={14} />
      <div>
        <b>Nicht im Ordner „Programme“.</b>
        {LOCATION_TEXT[status.location]} TippIT beenden, in „Programme“ ziehen und
        von dort starten.
      </div>
    </div>
  {/if}

  {#if show("permDiag")}
    <details class="trouble" open={!status.input_trusted || q !== ""}>
      <summary>
        <Icon name="chevron-down" size={12} />Fehlerbehebung
      </summary>
      <div class="card">
        <div class="row">
          <span class="row-label">
            Systemeinstellungen
            <span class="row-hint">Bedienungshilfen von Hand prüfen</span>
          </span>
          <button class="btn" disabled={busy} onclick={openSettings} type="button">
            <Icon name="external" size={14} />Öffnen
          </button>
        </div>
        <div class="row">
          <span class="row-label">
            Eintrag reparieren
            <span class="row-hint">Wenn TippIT dort „An“ steht und trotzdem nicht tippt</span>
          </span>
          <button class="btn" disabled={busy} onclick={repair} type="button">
            <Icon name="restore" size={14} />Reparieren
          </button>
        </div>
        {#if repairError}
          <div class="row stack">
            <span class="err-line">
              <Icon name="alert" size={13} />Ging nicht. Im Terminal ausführen,
              dann „Freigeben“:
            </span>
            <div class="inline">
              <code class="cmd">{repairCommand}</code>
              <button class="btn" onclick={() => copy(repairCommand)} type="button">
                <Icon name={copied ? "check" : "copy"} size={14} />Kopieren
              </button>
            </div>
          </div>
        {/if}
        <div class="row">
          <span class="row-label fill">
            Diagnose
            <span class="row-hint diag">{diagnosis}</span>
          </span>
          <button class="btn" onclick={() => copy(diagnosis)} type="button">
            <Icon name={copied ? "check" : "copy"} size={14} />Kopieren
          </button>
        </div>
      </div>
    </details>
  {/if}
{:else if !status && show("permAx")}
  <div class="card"><div class="row"><span class="row-hint">Wird geprüft…</span></div></div>
{/if}

<style>
  .state {
    display: inline-flex;
    flex: none;
    gap: 5px;
    align-items: center;
    height: 24px;
    padding: 0 10px;
    font: 500 var(--fs-button) / 1 var(--font-ui);
    color: var(--success);
    background: var(--success-soft);
    border-radius: var(--r-full);
  }
  .notice {
    display: flex;
    gap: 10px;
    align-items: flex-start;
    padding: 10px 14px;
    margin-top: 8px;
    font-size: var(--fs-control);
    line-height: 1.45;
    color: var(--warn);
    background: var(--warn-soft);
    border-radius: var(--r-lg);
  }
  .notice :global(.ic) {
    flex: none;
    margin-top: 2px;
  }
  .notice div {
    color: var(--fg-body);
  }
  .trouble {
    margin-top: 10px;
  }
  .trouble summary {
    display: inline-flex;
    gap: 6px;
    align-items: center;
    padding: 4px 4px 8px;
    font: 500 var(--fs-control) / 1 var(--font-ui);
    color: var(--fg-muted);
    cursor: pointer;
    list-style: none;
  }
  .trouble summary::-webkit-details-marker {
    display: none;
  }
  .trouble summary :global(.ic) {
    transform: rotate(-90deg);
    transition: transform var(--t-fast) ease;
  }
  .trouble[open] summary :global(.ic) {
    transform: rotate(0deg);
  }
  .trouble summary:hover {
    color: var(--fg);
  }
  .cmd {
    flex: 1;
    min-width: 0;
    padding: 7px 10px;
    font: 400 var(--fs-meta) / 1.3 var(--font-mono);
    color: var(--fg-body);
    user-select: text;
    background: var(--bg-base);
    border-radius: var(--r-md);
  }
  .fill {
    flex: 1;
    min-width: 0;
  }
  .diag {
    max-width: none;
    font-family: var(--font-mono);
    word-break: break-all;
    white-space: pre-wrap;
    user-select: text;
  }
</style>
