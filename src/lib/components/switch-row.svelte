<script lang="ts">
  // Einstellungszeile mit Ein-/Aus-Schalter; die ganze Zeile ist Klickfläche.
  interface Props {
    checked: boolean;
    hint?: string;
    label: string;
    onchange: () => void;
  }
  let { checked = $bindable(), hint, label, onchange }: Props = $props();
</script>

<label class="row">
  <span class="row-label">
    {label}
    {#if hint}
      <span class="row-hint">{hint}</span>
    {/if}
  </span>
  <span class="switch">
    <input
      aria-checked={checked}
      {onchange}
      role="switch"
      type="checkbox"
      bind:checked
    >
    <span class="track"></span>
    <span class="knob"></span>
  </span>
</label>

<style>
  .switch {
    position: relative;
    flex: none;
    width: 34px;
    height: 20px;
  }
  input {
    position: absolute;
    inset: 0;
    margin: 0;
    cursor: pointer;
    opacity: 0;
  }
  .track {
    display: block;
    width: 34px;
    height: 20px;
    background: var(--bg-strong);
    border-radius: var(--r-full);
    transition: background var(--t-base) linear;
  }
  .knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 16px;
    height: 16px;
    background: var(--fg-on-accent);
    border-radius: var(--r-round);
    transition: transform var(--t-base) ease-out;
  }
  input:checked ~ .track {
    background: var(--accent);
  }
  input:checked ~ .knob {
    transform: translateX(14px);
  }
  input:focus-visible ~ .track {
    box-shadow: var(--shadow-focus);
  }
</style>
