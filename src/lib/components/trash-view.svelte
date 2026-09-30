<script lang="ts">
  import type { TrashState } from "$lib/history/trash.svelte";
  import Icon from "$lib/icon.svelte";
  import { maskOtpauthSecret } from "$lib/totp";

  interface Props {
    /** id der Liste, auf die das Suchfeld per aria-controls zeigt. */
    listId: string;
    trash: TrashState;
  }

  const { trash, listId }: Props = $props();
</script>

<section class="list">
  <div class="trash-bar">
    <span class="trash-note"> Gelöschtes bleibt 30 Tage liegen. </span>
    <button
      class="link-btn danger"
      disabled={trash.items.length === 0}
      onclick={() => trash.empty()}
      type="button"
    >
      Endgültig leeren
    </button>
  </div>
  {#if trash.items.length === 0}
    <p class="empty">Der Papierkorb ist leer.</p>
  {:else}
    <div aria-label="Papierkorb" id={listId} role="listbox" tabindex="-1">
      {#each trash.items as item, i (item.uuid)}
        <div
          aria-selected={i === trash.selected}
          class="row"
          id="t-{item.uuid}"
          onmousemove={() => (trash.selected = i)}
          role="option"
          tabindex="-1"
          class:selected={i === trash.selected}
        >
          <span class="preview">{maskOtpauthSecret(item.preview)}</span>
          <button
            aria-label="Wiederherstellen"
            class="row-act"
            onclick={() => trash.restore(item.uuid)}
            title="Wiederherstellen (Enter)"
            type="button"
          >
            <Icon name="restore" size={13} />
          </button>
          <button
            aria-label="Endgültig löschen"
            class="row-act danger"
            onclick={() => trash.purge(item.uuid)}
            title="Endgültig löschen"
            type="button"
          >
            <Icon name="x" size={13} />
          </button>
        </div>
      {/each}
    </div>
  {/if}
</section>

<style>
  .list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }
  .trash-bar {
    display: flex;
    gap: var(--s-4);
    align-items: center;
    justify-content: space-between;
    padding: var(--s-3) var(--s-6);
    border-bottom: 1px solid var(--border-soft);
  }
  .trash-note {
    font-size: var(--fs-micro);
    color: var(--fg-dim);
  }
  /* Gleiche Zeilenform wie die Historie-Liste (history-list.svelte). */
  .row {
    display: flex;
    gap: var(--s-2);
    align-items: center;
    height: var(--row-h);
    padding: 0 var(--s-4);
    margin: 0 var(--s-3);
    cursor: default;
    border-radius: var(--r-sm);
  }
  .row.selected {
    background: var(--bg-hover);
  }
  .preview {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--fg-body);
    white-space: nowrap;
  }
  .row.selected .preview {
    color: var(--fg);
  }
  .row-act {
    display: grid;
    flex: none;
    place-items: center;
    width: 22px;
    height: 22px;
    color: var(--fg-dim);
    cursor: pointer;
    background: transparent;
    border: 0;
    border-radius: var(--r-xs);
  }
  .row-act:hover {
    color: var(--fg-body);
    background: var(--row-hover);
  }
  .row-act.danger:hover {
    color: var(--danger);
  }
  .empty {
    margin-top: 48px;
    color: var(--fg-dim);
    text-align: center;
  }
  .link-btn {
    padding: 2px 4px;
    font-size: var(--fs-meta);
    color: var(--accent-text);
    cursor: pointer;
    background: transparent;
    border: 0;
  }
  .link-btn:hover {
    text-decoration: underline;
  }
  .link-btn.danger {
    color: var(--danger);
  }
</style>
