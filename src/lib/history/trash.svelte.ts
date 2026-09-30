import {
  emptyTrash,
  listTrash,
  purgeEntry,
  restoreEntry,
  type TrashDto,
} from "../api";

/**
 * Papierkorb-Ansicht der Historie: eigene Liste, eigene Auswahl, eigene
 * Tasten. Die Einträge dort sind nicht im Suchindex und tragen andere
 * Aktionen, deshalb ist es kein Filter.
 */
export class TrashState {
  items = $state<TrashDto[]>([]);
  selected = $state(0);
  readonly current = $derived(this.items[this.selected]);

  async load(): Promise<void> {
    try {
      this.items = await listTrash();
      if (this.selected >= this.items.length) {
        this.selected = Math.max(0, this.items.length - 1);
      }
    } catch {
      this.items = [];
    }
  }

  open(): Promise<void> {
    this.selected = 0;
    return this.load();
  }

  async restore(uuid: string): Promise<void> {
    await restoreEntry(uuid).catch(() => {
      // Rust-Log
    });
    await this.load();
  }

  async purge(uuid: string): Promise<void> {
    await purgeEntry(uuid).catch(() => {
      // Rust-Log
    });
    await this.load();
  }

  async empty(): Promise<void> {
    await emptyTrash().catch(() => {
      // Rust-Log
    });
    await this.load();
  }

  /** Pfeile bewegen die Auswahl, Enter stellt wieder her. */
  async handleKey(e: KeyboardEvent): Promise<void> {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      this.selected = Math.max(
        0,
        Math.min(this.selected + 1, this.items.length - 1)
      );
      this.scrollToSelected();
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      this.selected = Math.max(this.selected - 1, 0);
      this.scrollToSelected();
    } else if (e.key === "Enter" && this.current) {
      e.preventDefault();
      await this.restore(this.current.uuid);
    }
  }

  private scrollToSelected(): void {
    const uuid = this.current?.uuid;
    if (uuid) {
      document
        .getElementById(`t-${uuid}`)
        ?.scrollIntoView({ block: "nearest" });
    }
  }
}
