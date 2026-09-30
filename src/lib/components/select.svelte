<script generics="T extends string | number" lang="ts">
  import Icon from "$lib/icon.svelte";

  interface Props {
    /** Für ein <label for> in der Zeile. */
    id: string;
    /** Läuft nach der Wertübernahme, bekommt den neuen Wert. */
    onchange: (value: T) => void;
    options: readonly { label: string; value: T }[];
    value: T;
  }
  let { id, onchange, options, value = $bindable() }: Props = $props();
</script>

<span class="select">
  <select {id} onchange={() => onchange(value)} bind:value>
    {#each options as option (option.value)}
      <option value={option.value}>{option.label}</option>
    {/each}
  </select>
  <Icon name="chevron-down" size={12} />
</span>

<style>
  .select {
    position: relative;
    display: inline-flex;
    flex: 0 1 auto;
    align-items: center;
    min-width: 0;
    color: var(--fg-dim);
  }
  select {
    min-width: 156px;
    max-width: 100%;
    height: 30px;
    padding: 0 30px 0 10px;
    overflow: hidden;
    text-overflow: ellipsis;
    font: 400 var(--fs-control) / 1 var(--font-ui);
    color: var(--fg-body);
    appearance: none;
    cursor: pointer;
    outline: none;
    background: var(--bg-raised);
    border: 0;
    border-radius: var(--r-md);
  }
  .select :global(.ic) {
    position: absolute;
    right: 9px;
    color: var(--fg-dim);
    pointer-events: none;
  }
  select:focus-visible {
    box-shadow: inset 0 0 0 1px var(--border-focus);
  }
</style>
