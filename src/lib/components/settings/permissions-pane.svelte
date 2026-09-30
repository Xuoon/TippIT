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
    /** Zeile sichtbar (aktiver Tab oder Suchtreffer)? */
    show: (key: string) => boolean;
  }
  const { show }: Props = $props();

  let status = $state<PermissionStatus | null>(null);
  let busy = $state(false);
  let repairError = $state("");
  let copied = $state(false);

  const LOCATION_TEXT: Record<
    Exclude<PermissionStatus["location"], "applications">,
    string
  > = {
    translocated:
      "macOS startet TippIT aus einem schreibgeschützten Zwischenordner, weil die App nicht im Ordner „Programme“ liegt. Updates und Autostart funktionieren so nicht.",
    dmg: "TippIT läuft direkt aus dem Installationsabbild (DMG). Updates und Autostart funktionieren so nicht.",
    downloads:
      "TippIT läuft aus dem Ordner „Downloads“. Damit Updates und Autostart verlässlich funktionieren, gehört TippIT in den Ordner „Programme“.",
    other:
      "TippIT liegt nicht im Ordner „Programme“. Damit Updates und Autostart verlässlich funktionieren, gehört TippIT dorthin.",
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
    <div class="row">
      <span class="row-label">
        Bedienungshilfen
        <span class="row-hint">
          Ohne diese Freigabe verwirft macOS alles, was TippIT tippt oder
          einfügt.
        </span>
      </span>
      <span class="state" class:ok={status.input_trusted}>
        <Icon name={status.input_trusted ? "check" : "alert"} size={13} />
        {status.input_trusted ? "Erteilt" : "Fehlt"}
      </span>
    </div>
    <div class="row actions">
      {#if !status.input_trusted}
        <button
          class="btn primary"
          disabled={busy}
          onclick={request}
          type="button"
        >
          Anfragen
        </button>
      {/if}
      <button class="btn" disabled={busy} onclick={openSettings} type="button">
        <Icon name="external" size={14} />Systemeinstellungen öffnen
      </button>
      <button class="btn" disabled={busy} onclick={repair} type="button">
        <Icon name="restore" size={14} />Eintrag reparieren
      </button>
    </div>
    <div class="row">
      <span class="row-hint wide">
        Steht TippIT in den Systemeinstellungen schon auf „An“, hier aber auf
        „Fehlt“, gehört der Eintrag zu einer früheren Version. „Eintrag
        reparieren“ entfernt ihn und fragt neu an.
      </span>
    </div>
    {#if repairError}
      <div class="row error-row">
        <Icon name="alert" size={14} />
        <span>
          Reparieren fehlgeschlagen. Im Terminal ausführen, dann „Anfragen“:
        </span>
      </div>
      <div class="row actions">
        <code class="cmd">{repairCommand}</code>
        <button class="btn" onclick={() => copy(repairCommand)} type="button">
          <Icon name={copied ? "check" : "copy"} size={14} />Kopieren
        </button>
      </div>
    {/if}
  {/if}

  {#if show("permAx") && status.clipboard_access === "deny"}
    <div class="row error-row">
      <Icon name="alert" size={14} />
      <span>
        macOS verweigert TippIT das Lesen der Zwischenablage, die Historie bleibt
        leer. Freigeben lässt es sich in den Systemeinstellungen unter
        Datenschutz &amp; Sicherheit.
      </span>
    </div>
  {/if}

  {#if show("permLocation") && status.location !== "applications"}
    <div class="row notice">
      <Icon name="alert" size={14} />
      <div>
        <p>{LOCATION_TEXT[status.location]}</p>
        <ol>
          <li>TippIT beenden (Tray-Symbol → Beenden).</li>
          <li>
            Im Finder <b>TippIT.app</b> in den Ordner „Programme“ ziehen{status.location ===
            "dmg"
              ? ", direkt aus dem Fenster des Installationsabbilds"
              : ""}.
          </li>
          <li>
            TippIT aus „Programme“ starten{status.location === "dmg"
              ? " und das Installationsabbild auswerfen"
              : ""}.
          </li>
        </ol>
      </div>
    </div>
  {/if}

  {#if show("permFirstRun")}
    <div class="row">
      <span class="row-label">
        Erster Start
        <span class="row-hint">
          Meldet macOS, TippIT könne nicht geöffnet werden: Systemeinstellungen
          → Datenschutz &amp; Sicherheit, ganz unten „Dennoch öffnen“ wählen.
        </span>
      </span>
    </div>
  {/if}

  {#if show("permDiag")}
    <div class="row">
      <span class="row-label fill">
        Diagnose
        <span class="row-hint diag">{diagnosis}</span>
      </span>
      <button class="btn" onclick={() => copy(diagnosis)} type="button">
        <Icon name={copied ? "check" : "copy"} size={14} />Kopieren
      </button>
    </div>
  {/if}
{:else if !status && show("permAx")}
  <div class="row"><span class="row-hint">Wird geprüft…</span></div>
{/if}

<style>
  .state {
    display: inline-flex;
    flex: none;
    gap: 5px;
    align-items: center;
    height: 22px;
    padding: 0 9px;
    font: 500 var(--fs-micro) / 1 var(--font-ui);
    color: var(--danger);
    background: var(--danger-soft);
    border-radius: var(--r-full);
  }
  .state.ok {
    color: var(--success);
    background: var(--success-soft);
  }
  .wide {
    max-width: none;
    margin: 6px 0;
  }
  .cmd {
    flex: 1 1 200px;
    padding: 7px 10px;
    font: 400 var(--fs-meta) / 1.3 var(--font-mono);
    color: var(--fg-body);
    user-select: text;
    background: var(--bg-raised);
    border-radius: var(--r-md);
  }
  .notice {
    align-items: flex-start;
    padding-top: 10px;
    padding-bottom: 10px;
    font-size: var(--fs-control);
    line-height: 1.45;
    color: var(--warn);
    background: var(--warn-soft);
  }
  .notice :global(.ic) {
    margin-top: 2px;
  }
  .notice div {
    flex: 1;
    color: var(--fg-body);
  }
  .notice p {
    margin: 0 0 4px;
  }
  .notice ol {
    padding-left: 18px;
    margin: 0;
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
