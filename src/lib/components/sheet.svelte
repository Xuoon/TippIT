<script lang="ts">
  import type { Snippet } from "svelte";

  interface Props {
    actions: Snippet;
    children: Snippet;
    title: string;
  }

  const { title, children, actions }: Props = $props();
  const titleId = $props.id();
</script>

<!-- Overlay über dem ganzen Fenster; Tasten (Esc, Speichern) behandelt die
     Route global, deshalb hier kein eigener Handler. -->
<div class="overlay">
  <div aria-labelledby={titleId} aria-modal="true" class="sheet" role="dialog">
    <div class="sheet-title" id={titleId}>{title}</div>
    {@render children()}
    <div class="sheet-acts">{@render actions()}</div>
  </div>
</div>

<style>
  .overlay {
    position: absolute;
    inset: 0;
    z-index: 10;
    display: grid;
    place-items: center;
    background: var(--overlay-scrim);
  }
  .sheet {
    display: flex;
    flex-direction: column;
    gap: var(--s-4);
    width: min(440px, 82%);
    padding: var(--s-6);
    background: var(--bg-raised);
    border: 1px solid var(--border);
    border-radius: var(--r-xl);
    box-shadow: var(--shadow-overlay);
  }
  .sheet-title {
    font: 600 var(--fs-control) / 1.2 var(--font-ui);
    color: var(--fg-body);
  }
  .sheet-acts {
    display: flex;
    gap: var(--s-4);
    justify-content: flex-end;
  }
</style>
