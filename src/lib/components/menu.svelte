<script lang="ts" module>
  export interface MenuItem {
    checked: boolean;
    disabled?: boolean;
    label: string;
    onselect: () => void;
    /** radio: eine Wahl aus der Gruppe, hinterlegt; check: Schalter, nur Haken. */
    role?: "check" | "radio";
    title?: string;
  }
</script>

<script lang="ts">
  interface Props {
    /** Am rechten (Standard) oder linken Rand des Auslösers ausrichten. */
    align?: "left" | "right";
    label: string;
    /** Gruppen, zwischen denen eine Trennlinie steht. */
    sections: MenuItem[][];
  }

  const { label, sections, align = "right" }: Props = $props();
</script>

{#snippet content(item: MenuItem)}
  <span class="check">{item.checked ? "✓" : ""}</span>
  {item.label}
{/snippet}

<!-- pointerdown bleibt im Menü: der Außenklick-Handler der Route würde es
     sonst vor dem click schließen. -->
<div
  aria-label={label}
  class="menu"
  onpointerdown={(e) => e.stopPropagation()}
  role="menu"
  tabindex="-1"
  class:left={align === "left"}
>
  {#each sections as section, s (s)}
    {#if s > 0}
      <hr class="sep">
    {/if}
    {#each section as item (item.label)}
      {#if item.role === "check"}
        <button
          aria-checked={item.checked}
          class="item"
          disabled={item.disabled}
          onclick={item.onselect}
          role="menuitemcheckbox"
          title={item.title}
          type="button"
        >
          {@render content(item)}
        </button>
      {:else}
        <button
          aria-checked={item.checked}
          class="item"
          disabled={item.disabled}
          onclick={item.onselect}
          role="menuitemradio"
          title={item.title}
          type="button"
          class:active={item.checked}
        >
          {@render content(item)}
        </button>
      {/if}
    {/each}
  {/each}
</div>

<style>
  .menu {
    position: absolute;
    top: calc(100% + 4px);
    right: 0;
    z-index: 20;
    min-width: 200px;
    padding: 6px;
    background: var(--bg-raised);
    border: 1px solid var(--border);
    border-radius: var(--r-lg);
    box-shadow: var(--shadow-overlay);
  }
  .menu.left {
    right: auto;
    left: 0;
  }
  .item {
    display: flex;
    gap: 8px;
    align-items: center;
    width: 100%;
    padding: 7px 10px;
    font: 450 var(--fs-control) / 1 var(--font-ui);
    color: var(--fg-body);
    text-align: left;
    cursor: pointer;
    background: transparent;
    border: 0;
    border-radius: var(--r-md);
  }
  .item:hover,
  .item.active {
    background: var(--row-hover);
  }
  .check {
    width: 14px;
    color: var(--accent-text);
  }
  .item:disabled {
    cursor: default;
    opacity: 0.45;
  }
  .sep {
    height: 1px;
    margin: 4px 6px;
    background: var(--border);
    border: 0;
  }
</style>
