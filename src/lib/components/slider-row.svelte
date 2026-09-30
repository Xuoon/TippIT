<script lang="ts">
  // Einstellungszeile mit Schieberegler; Doppelklick setzt auf den
  // Auslieferungs-Default zurück (onreset).
  interface Props {
    hint?: string;
    label: string;
    max: number;
    min: number;
    onchange: () => void;
    onreset: () => void;
    step?: number;
    unit?: string;
    value: number;
  }
  let {
    hint,
    label,
    max,
    min,
    onchange,
    onreset,
    step = 1,
    unit = "",
    value = $bindable(),
  }: Props = $props();
</script>

<label class="row slider">
  <span class="row-label">
    {label}
    {#if hint}
      <span class="row-hint">{hint}</span>
    {/if}
  </span>
  <input
    {max}
    {min}
    {onchange}
    ondblclick={onreset}
    {step}
    title="Doppelklick: Standard"
    type="range"
    bind:value
  >
  <output>{unit ? `${value} ${unit}` : value}</output>
</label>

<style>
  /* Feste Breite, rechtsbündig: alle Regler beginnen an derselben Kante,
     egal wie lang das Label ist. */
  input {
    flex: 0 1 240px;
    min-width: 0;
    height: 4px;
    margin: 0 0 0 auto;
    accent-color: var(--accent);
  }
  output {
    flex: none;
    min-width: 58px;
    font: 400 var(--fs-button) / 1 var(--font-ui);
    font-variant-numeric: tabular-nums;
    color: var(--accent-text);
    text-align: right;
  }
</style>
