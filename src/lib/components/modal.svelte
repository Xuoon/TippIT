<script lang="ts">
  import { onMount, type Snippet } from "svelte";
  import Icon from "$lib/icon.svelte";

  // Esc schließt nicht hier, sondern im Tasten-Handler der Route: der kennt
  // alle Overlays und schließt ohne offenes Overlay das Fenster.
  interface Props {
    children: Snippet;
    /** Kleiner Dialog über abgedunkeltem Fenster statt ganzer Fensterfläche. */
    compact?: boolean;
    onclose: () => void;
    title: string;
  }
  const { children, compact = false, onclose, title }: Props = $props();
  const titleId = $props.id();

  let dialog = $state<HTMLDivElement>();
  let closeButton = $state<HTMLButtonElement>();

  onMount(() => {
    const opener =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;
    closeButton?.focus();
    return () => opener?.focus();
  });

  const FOCUSABLE =
    'button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea, summary, [href], [tabindex]:not([tabindex="-1"])';

  // aria-modal allein hält den Fokus nicht im Dialog.
  function keepFocusInside(event: KeyboardEvent) {
    if (event.key !== "Tab" || !dialog) {
      return;
    }
    const items = [...dialog.querySelectorAll<HTMLElement>(FOCUSABLE)];
    const first = items[0];
    const last = items.at(-1);
    if (!(first && last)) {
      return;
    }
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  }
</script>

<svelte:window onkeydown={keepFocusInside} />

<div class="backdrop" class:compact={compact}>
  <div
    aria-labelledby={titleId}
    aria-modal="true"
    class="modal"
    role="dialog"
    tabindex="-1"
    bind:this={dialog}
  >
    <div class="head">
      <h2 id={titleId}>{title}</h2>
      <button
        aria-label="Schließen"
        class="close"
        onclick={onclose}
        title="Schließen (Esc)"
        type="button"
        bind:this={closeButton}
      >
        <Icon name="x" size={14} />
      </button>
    </div>
    <div class="body">{@render children()}</div>
  </div>
</div>

<style>
  /* Vollflächig füllt das Overlay das ganze Fenster; Rahmen, Radius und
     Schatten entfallen, weil nichts mehr dahinter sichtbar ist. */
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 10;
    display: flex;
    background: var(--bg-base);
  }
  .modal {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
    overflow: hidden;
    outline: none;
    background: var(--bg-base);
  }
  .backdrop.compact {
    align-items: center;
    justify-content: center;
    padding: var(--s-8);
    background: var(--overlay-scrim);
  }
  .compact .modal {
    flex: 0 1 380px;
    max-height: 100%;
    border: 1px solid var(--border);
    border-radius: var(--r-xl);
    box-shadow: var(--shadow-overlay);
  }
  .head {
    display: flex;
    flex: none;
    align-items: center;
    justify-content: space-between;
    padding: 10px 14px;
    border-bottom: 1px solid var(--border);
  }
  .compact .head {
    border-bottom: 0;
  }
  h2 {
    margin: 0;
    font: 600 var(--fs-title) / 1 var(--font-ui);
    color: var(--fg);
  }
  .close {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    color: var(--fg-dim);
    cursor: pointer;
    background: transparent;
    border: 0;
    border-radius: var(--r-md);
  }
  .close:hover {
    color: var(--fg);
    background: var(--row-hover);
  }
  .close:focus-visible {
    outline: none;
    box-shadow: var(--shadow-focus);
  }
  .body {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-height: 0;
    padding: 4px 14px 14px;
    overflow-y: auto;
    user-select: text;
  }
</style>
