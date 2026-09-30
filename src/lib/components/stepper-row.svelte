<script lang="ts">
  // Einstellungszeile mit Zahlenwert: −/+ rasten auf `step`, das Feld nimmt
  // jeden Wert im Bereich; Doppelklick auf den Wert setzt den Standard.
  import { clampValue, stepValue } from "$lib/components/settings/stepper";

  interface Props {
    disabled?: boolean;
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
    disabled = false,
    label,
    max,
    min,
    onchange,
    onreset,
    step = 1,
    unit = "",
    value = $bindable(),
  }: Props = $props();
  const id = $props.id();

  function set(next: number) {
    if (next !== value) {
      value = next;
      onchange();
    }
  }

  function commit(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    const next = clampValue(input.valueAsNumber, min, max);
    input.value = String(next);
    set(next);
  }
</script>

<div class="row" class:disabled>
  <label class="row-label" for={id}>{label}</label>
  <span class="stepper">
    <button
      aria-label="{label} verringern"
      disabled={disabled || value <= min}
      onclick={() => set(stepValue(value, -1, step, min, max))}
      tabindex="-1"
      type="button"
    >
      −
    </button>
    <input
      {disabled}
      {id}
      {max}
      {min}
      onchange={commit}
      ondblclick={onreset}
      onkeydown={(e) => {
        if (e.key === "Enter") {
          (e.currentTarget as HTMLInputElement).blur();
        }
      }}
      title="Doppelklick: Standard"
      type="number"
      value={value}
    >
    {#if unit}
      <span class="unit">{unit}</span>
    {/if}
    <button
      aria-label="{label} erhöhen"
      disabled={disabled || value >= max}
      onclick={() => set(stepValue(value, 1, step, min, max))}
      tabindex="-1"
      type="button"
    >
      +
    </button>
  </span>
</div>

<style>
  .disabled .row-label {
    color: var(--fg-disabled);
  }
  .stepper {
    display: inline-flex;
    flex: none;
    align-items: center;
    height: 26px;
    background: var(--bg-base);
    border-radius: var(--r-md);
  }
  .stepper:focus-within {
    box-shadow: inset 0 0 0 1px var(--border-focus);
  }
  button {
    display: grid;
    place-items: center;
    width: 24px;
    height: 26px;
    padding: 0;
    font: 500 var(--fs-control) / 1 var(--font-ui);
    color: var(--fg-muted);
    cursor: pointer;
    background: transparent;
    border: 0;
    border-radius: var(--r-md);
  }
  button:hover {
    color: var(--fg);
    background: var(--row-hover);
  }
  button:disabled {
    cursor: default;
    opacity: 0.35;
  }
  input {
    width: 40px;
    padding: 0;
    font: 400 var(--fs-button) / 1 var(--font-ui);
    font-variant-numeric: tabular-nums;
    color: var(--accent-text);
    text-align: right;
    appearance: textfield;
    outline: none;
    background: transparent;
    border: 0;
  }
  input::-webkit-inner-spin-button,
  input::-webkit-outer-spin-button {
    margin: 0;
    appearance: none;
  }
  input:disabled {
    color: var(--fg-disabled);
  }
  .unit {
    padding-left: 3px;
    font: 400 var(--fs-button) / 1 var(--font-ui);
    color: var(--fg-dim);
  }
</style>
