<script lang="ts">
  // Gemeinsames Icon-Set: alle Symbole als Inline-SVG, viewBox 0 0 16 16,
  // stroke=currentColor, stroke-width 1.5, runde Kappen. Farbe kommt vom Elternteil
  // (currentColor), Größe über die size-Prop. Keine Emoji.
  interface Props {
    name: string;
    size?: number;
  }
  const { name, size = 16 }: Props = $props();
</script>

<svg
  aria-hidden="true"
  class="ic"
  fill="none"
  focusable="false"
  height={size}
  stroke="currentColor"
  stroke-linecap="round"
  stroke-linejoin="round"
  stroke-width="1.5"
  viewBox="0 0 16 16"
  width={size}
>
  {#if name === "search"}
    <circle cx="7" cy="7" r="4.75" />
    <path d="M10.6 10.6 13.8 13.8" />
  {:else if name === "x"}
    <path d="M4.5 4.5 11.5 11.5M11.5 4.5 4.5 11.5" />
  {:else if name === "star"}
    <path
      d="M8 1.8 9.53 6.3 14.28 6.36 10.47 9.2 11.88 13.74 8 11 4.12 13.74 5.53 9.2 1.72 6.36 6.47 6.3Z"
    />
  {:else if name === "star-filled"}
    <path
      d="M8 1.8 9.53 6.3 14.28 6.36 10.47 9.2 11.88 13.74 8 11 4.12 13.74 5.53 9.2 1.72 6.36 6.47 6.3Z"
      fill="currentColor"
    />
  {:else if name === "copy"}
    <path
      d="M7.5 6h5A1.5 1.5 0 0 1 14 7.5v5A1.5 1.5 0 0 1 12.5 14h-5A1.5 1.5 0 0 1 6 12.5v-5A1.5 1.5 0 0 1 7.5 6Z"
    />
    <path
      d="M4 10H3.5A1.5 1.5 0 0 1 2 8.5v-5A1.5 1.5 0 0 1 3.5 2h5A1.5 1.5 0 0 1 10 3.5V4"
    />
  {:else if name === "keyboard"}
    <path
      d="M2.75 4.5h10.5A1.25 1.25 0 0 1 14.5 5.75v4.5A1.25 1.25 0 0 1 13.25 11.5H2.75A1.25 1.25 0 0 1 1.5 10.25v-4.5A1.25 1.25 0 0 1 2.75 4.5Z"
    />
    <path d="M4 7h.01M6.5 7h.01M9.5 7h.01M12 7h.01" />
    <path d="M5.5 9.25h5" />
  {:else if name === "trash"}
    <path d="M2.5 4.5h11" />
    <path
      d="M6 4.5V3.25A1.25 1.25 0 0 1 7.25 2h1.5A1.25 1.25 0 0 1 10 3.25V4.5"
    />
    <path
      d="M12.5 4.5v8.25A1.25 1.25 0 0 1 11.25 14h-6.5A1.25 1.25 0 0 1 3.5 12.75V4.5"
    />
    <path d="M6.75 7.25v3.5M9.25 7.25v3.5" />
  {:else if name === "text"}
    <path d="M3 4.5h10M3 8h10M3 11.5h6.5" />
  {:else if name === "image"}
    <path
      d="M3.25 2.5h9.5A1.25 1.25 0 0 1 14 3.75v8.5A1.25 1.25 0 0 1 12.75 13.5h-9.5A1.25 1.25 0 0 1 2 12.25v-8.5A1.25 1.25 0 0 1 3.25 2.5Z"
    />
    <circle cx="5.75" cy="6" r="1" />
    <path d="M2 11.5 5.75 7.75l2.25 2.25 2.5-2.5L14 11" />
  {:else if name === "file"}
    <path
      d="M9 1.75H4.75A1.25 1.25 0 0 0 3.5 3v10A1.25 1.25 0 0 0 4.75 14.25h6.5A1.25 1.25 0 0 0 12.5 13V5.25Z"
    />
    <path d="M9 1.75V5.25h3.5" />
  {:else if name === "clipboard"}
    <path
      d="M5.5 3H4.25A1.25 1.25 0 0 0 3 4.25v9.5A1.25 1.25 0 0 0 4.25 15h7.5A1.25 1.25 0 0 0 13 13.75v-9.5A1.25 1.25 0 0 0 11.75 3H10.5"
    />
    <path
      d="M6.25 1.5h3.5a.75.75 0 0 1 .75.75V3.5a.75.75 0 0 1-.75.75h-3.5a.75.75 0 0 1-.75-.75V2.25a.75.75 0 0 1 .75-.75Z"
    />
  {:else if name === "link"}
    <path d="M6.5 9.5 9.5 6.5" />
    <path
      d="M7.4 4.9 9 3.3a2.25 2.25 0 0 1 3.18 0l.52.52a2.25 2.25 0 0 1 0 3.18L11.1 8.6"
    />
    <path
      d="M8.6 11.1 7 12.7a2.25 2.25 0 0 1-3.18 0l-.52-.52a2.25 2.25 0 0 1 0-3.18L4.9 7.4"
    />
  {:else if name === "help"}
    <circle cx="8" cy="8" r="6.25" />
    <path d="M6.3 6.1a1.75 1.75 0 1 1 2.55 1.7c-.5.28-.85.62-.85 1.2v.2" />
    <path d="M8 11.3v.02" />
  {:else if name === "chevron-down"}
    <path d="M4.5 6.25 8 9.75l3.5-3.5" />
  {:else if name === "check"}
    <path d="M3.25 8.5 6.5 11.75 12.75 5.5" />
  {:else if name === "alert"}
    <path
      d="M7.13 2.6 1.6 12.25a1 1 0 0 0 .87 1.5h11.06a1 1 0 0 0 .87-1.5L8.87 2.6a1 1 0 0 0-1.74 0Z"
    />
    <path d="M8 6.5v3M8 11.75h.01" />
  {:else if name === "sliders"}
    <path d="M2.5 5h11M2.5 11h11" />
    <circle cx="6" cy="5" r="1.5" />
    <circle cx="10.5" cy="11" r="1.5" />
  {:else if name === "cursor-text"}
    <path d="M5.75 2.5h4.5M5.75 13.5h4.5M8 2.5v11" />
  {:else if name === "clock"}
    <circle cx="8" cy="8" r="6" />
    <path d="M8 4.75V8l2.25 1.5" />
  {:else if name === "sync"}
    <path d="M14 8a6 6 0 0 0-6-6 6.5 6.5 0 0 0-4.49 1.83L2 5.33" />
    <path d="M2 2v3.33h3.33" />
    <path d="M2 8a6 6 0 0 0 6 6 6.5 6.5 0 0 0 4.49-1.83L14 10.67" />
    <path d="M10.67 10.67H14V14" />
  {:else if name === "download"}
    <path d="M8 2v7.5" />
    <path d="M5 7 8 10l3-3" />
    <path
      d="M2.5 12.5v.75A1.25 1.25 0 0 0 3.75 14.5h8.5a1.25 1.25 0 0 0 1.25-1.25v-.75"
    />
  {:else if name === "app"}
    <path
      d="M4.25 2.5h2.5A1.25 1.25 0 0 1 8 3.75v2.5A1.25 1.25 0 0 1 6.75 7.5h-2.5A1.25 1.25 0 0 1 3 6.25v-2.5A1.25 1.25 0 0 1 4.25 2.5Z"
    />
    <path
      d="M9.25 2.5h2.5A1.25 1.25 0 0 1 13 3.75v2.5A1.25 1.25 0 0 1 11.75 7.5h-2.5A1.25 1.25 0 0 1 8 6.25v-2.5A1.25 1.25 0 0 1 9.25 2.5Z"
    />
    <path
      d="M4.25 8.5h2.5A1.25 1.25 0 0 1 8 9.75v2.5A1.25 1.25 0 0 1 6.75 13.5h-2.5A1.25 1.25 0 0 1 3 12.25v-2.5A1.25 1.25 0 0 1 4.25 8.5Z"
    />
    <path
      d="M9.25 8.5h2.5A1.25 1.25 0 0 1 13 9.75v2.5A1.25 1.25 0 0 1 11.75 13.5h-2.5A1.25 1.25 0 0 1 8 12.25v-2.5A1.25 1.25 0 0 1 9.25 8.5Z"
    />
  {:else if name === "key"}
    <circle cx="5.75" cy="10.25" r="2.5" />
    <path d="M7.6 8.4 13.5 2.5M11.25 2.5h2.25V4.75" />
  {:else if name === "sort"}
    <path d="M4 5h8M4 8h5.5M4 11h3" />
    <path d="M11 9.5v4M9.5 12 11 13.5 12.5 12" />
  {:else if name === "panel-preview"}
    <path
      d="M2.5 3.25h11A1.25 1.25 0 0 1 14.75 4.5v7A1.25 1.25 0 0 1 13.5 12.75h-11A1.25 1.25 0 0 1 1.25 11.5v-7A1.25 1.25 0 0 1 2.5 3.25Z"
    />
    <path d="M9.5 3.25v9.5" />
  {:else if name === "panel-meta"}
    <path
      d="M2.5 3.25h11A1.25 1.25 0 0 1 14.75 4.5v7A1.25 1.25 0 0 1 13.5 12.75h-11A1.25 1.25 0 0 1 1.25 11.5v-7A1.25 1.25 0 0 1 2.5 3.25Z"
    />
    <path d="M4 9.5h8M4 11.25h5" />
  {:else if name === "scan"}
    <path d="M3 5.5V3.75A1.25 1.25 0 0 1 4.25 2.5H6" />
    <path d="M10 2.5h1.75A1.25 1.25 0 0 1 13 3.75V5.5" />
    <path d="M13 10.5v1.75A1.25 1.25 0 0 1 11.75 13.5H10" />
    <path d="M6 13.5H4.25A1.25 1.25 0 0 1 3 12.25V10.5" />
    <path d="M2.5 8h11" />
  {:else if name === "arrow-up"}
    <path d="M8 12.5V3.5M4.5 7 8 3.5 11.5 7" />
  {:else if name === "arrow-down"}
    <path d="M8 3.5v9M4.5 9 8 12.5 11.5 9" />
  {:else if name === "return"}
    <path d="M3.5 8h7.5A2 2 0 0 0 13 6V4.5" />
    <path d="M6 5 3.5 8 6 11" />
  {:else if name === "external"}
    <path
      d="M8 3H4.25A1.25 1.25 0 0 0 3 4.25v7.5A1.25 1.25 0 0 0 4.25 13h7.5A1.25 1.25 0 0 0 13 11.75V8"
    />
    <path d="M9.5 2.5h4v4M13 3 7.75 8.25" />
  {/if}
</svg>

<style>
  .ic {
    display: block;
    flex: none;
  }
</style>
