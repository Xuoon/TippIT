<script lang="ts">
  import { onMount } from "svelte";
  import {
    onSemanticStatus,
    type SemanticStatus,
    semanticCleanup,
    semanticDelete,
    semanticDisable,
    semanticEnable,
    semanticStatus,
  } from "#lib/api.ts";
  import SwitchRow from "#lib/components/switch-row.svelte";
  import { fmtBytes } from "#lib/format.ts";
  import Icon from "#lib/icon.svelte";
  import { phasePercent, phaseText } from "./semantic.ts";

  let status = $state<SemanticStatus | null>(null);
  let enabled = $state(false);
  let busy = $state(false);
  let errorText = $state("");
  let confirmDelete = $state(false);

  function apply(next: SemanticStatus) {
    status = next;
    // Während einer Aktion zeigt der Schalter das Gewünschte; Zwischenstände
    // aus Events setzen ihn nicht zurück.
    if (!busy) {
      enabled = next.enabled;
    }
  }

  /** Eine Aktion zur Zeit: der Schalter und die Knöpfe sind solange gesperrt. */
  async function run(action: () => Promise<SemanticStatus>) {
    if (busy) {
      return;
    }
    busy = true;
    errorText = "";
    try {
      const next = await action();
      busy = false;
      apply(next);
    } catch (e) {
      busy = false;
      errorText = String(e);
      enabled = status?.enabled ?? false;
    }
  }

  const toggle = () =>
    run(() => (enabled ? semanticEnable() : semanticDisable()));

  const percent = $derived(status ? phasePercent(status) : null);
  /** Veraltete Vektoren oder ältere Modellrevisionen auf der Platte. */
  const stale = $derived(
    status
      ? status.vectors.stale > 0 || status.outdated_model_bytes > 0
      : false
  );

  onMount(() => {
    semanticStatus()
      .then(apply)
      .catch((e) => (errorText = String(e)));
    const stop = onSemanticStatus(apply);
    return () => {
      stop.then((f) => f());
    };
  });
</script>

{#if status}
  <div class="card">
    <SwitchRow
      disabled={busy}
      label="Suche nach Bedeutung"
      onchange={toggle}
      bind:checked={enabled}
    />
    <div class="row">
      <span class="row-label">
        Modell
        <span class="row-hint">Granite Embedding, mehrsprachig, läuft auf der CPU</span>
      </span>
      <span class="value">
        {fmtBytes(status.download_bytes)} Download · ca. {fmtBytes(status.model_ram_bytes)} RAM
      </span>
    </div>
    <div class="row stack">
      <span class="row-label">Status</span>
      <span aria-live="polite" class="status" role="status" class:err={status.phase === "error"} >
        {phaseText(status)}
      </span>
      {#if percent !== null}
        <span
          aria-label="Fortschritt"
          aria-valuemax={100}
          aria-valuemin={0}
          aria-valuenow={Math.round(percent)}
          class="bar"
          role="progressbar"
        ><span style:width="{percent}%"></span></span>
      {/if}
    </div>
  </div>

  <div class="card">
    <div class="row">
      <span class="row-label">Modell auf der Platte</span>
      <span class="value">{fmtBytes(status.model_bytes)}</span>
    </div>
    <div class="row">
      <span class="row-label">
        Vektoren
        <span class="row-hint">verschlüsselt in der Historie-Datenbank</span>
      </span>
      <span class="value">
        {status.vectors.current} von {status.embeddable} Einträgen ·
        {fmtBytes(status.vectors.bytes)}
      </span>
    </div>
    <div class="row">
      <span class="row-label">Arbeitsspeicher</span>
      <span class="value">
        {status.loaded ? `ca. ${fmtBytes(status.ram_bytes)}` : `${fmtBytes(status.ram_bytes)} (Modell ruht)`}
      </span>
    </div>
    <div class="row">
      <span class="row-label">
        Bereinigen
        <span class="row-hint">Entfernt veraltete Vektoren und ältere Modellversionen</span>
      </span>
      <button
        class="btn"
        disabled={busy || !stale}
        onclick={() => run(semanticCleanup)}
        type="button"
      >
        <Icon name="check" size={14} />Bereinigen
      </button>
    </div>
    <div class="row">
      <span class="row-label">
        Alles löschen
        <span class="row-hint">Schaltet aus, löscht Modell und Vektoren. Die Historie bleibt.</span>
      </span>
      {#if confirmDelete}
        <span class="confirm">
          <button class="btn" onclick={() => (confirmDelete = false)} type="button">
            Abbrechen
          </button>
          <button
            class="btn danger solid"
            disabled={busy}
            onclick={() => {
              confirmDelete = false;
              run(semanticDelete);
            }}
            type="button"
          >
            <Icon name="trash" size={14} />Löschen
          </button>
        </span>
      {:else}
        <button
          class="btn danger"
          disabled={busy || (status.model_bytes === 0 && status.vectors.bytes === 0)}
          onclick={() => (confirmDelete = true)}
          type="button"
        >
          <Icon name="trash" size={14} />Löschen…
        </button>
      {/if}
    </div>
  </div>
  {#if errorText}
    <span class="err-line" role="alert"><Icon name="alert" size={13} />{errorText}</span>
  {/if}
  <p class="note">
    Alles läuft auf diesem Gerät. Heruntergeladen werden nur die Modelldateien
    (IBM Granite über Hugging Face, geprüfte Prüfsumme); aus der Historie verlässt nichts den
    Rechner. Treffer nach Bedeutung stehen unter den Worttreffern und tragen
    den Hinweis „ähnlich“.
  </p>
{:else if errorText}
  <span class="err-line" role="alert"><Icon name="alert" size={13} />{errorText}</span>
{/if}

<style>
  .value {
    font-size: var(--fs-control);
    color: var(--fg-dim);
    text-align: right;
  }
  .status {
    font-size: var(--fs-control);
    color: var(--fg-body);
  }
  .status.err {
    color: var(--danger);
  }
  .bar {
    display: block;
    height: 4px;
    overflow: hidden;
    background: var(--bg-strong);
    border-radius: var(--r-full);
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--accent);
    transition: width var(--t-base) linear;
  }
  .confirm {
    display: inline-flex;
    flex: none;
    gap: 6px;
    margin-left: auto;
  }
  .note {
    margin: 10px 2px 0;
    font-size: var(--fs-meta);
    line-height: 1.5;
    color: var(--fg-dim);
  }
</style>
