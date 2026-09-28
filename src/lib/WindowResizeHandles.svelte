<script>
  import { onMount, onDestroy } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";

  const appWindow = getCurrentWindow();
  const edges = [
    "North",
    "South",
    "East",
    "West",
    "NorthEast",
    "NorthWest",
    "SouthEast",
    "SouthWest",
  ];

  let enabled = true;
  let unlisten;

  async function syncState() {
    try {
      const [maximized, fullscreen] = await Promise.all([
        appWindow.isMaximized(),
        appWindow.isFullscreen(),
      ]);
      enabled = !maximized && !fullscreen;
    } catch (e) {
      enabled = true;
    }
  }

  function startResize(event, direction) {
    if (event.button !== 0) return;
    event.preventDefault();
    appWindow.startResizeDragging(direction);
  }

  onMount(async () => {
    await syncState();
    unlisten = await appWindow.onResized(syncState);
  });

  onDestroy(() => {
    unlisten?.();
  });
</script>

{#if enabled}
  {#each edges as edge}
    <div
      class="resize-handle {edge.toLowerCase()}"
      on:mousedown={(e) => startResize(e, edge)}
    ></div>
  {/each}
{/if}

<style>
  .resize-handle {
    position: fixed;
    z-index: 100000;
  }

  .north,
  .south {
    left: 8px;
    right: 8px;
    height: 5px;
  }

  .east,
  .west {
    top: 8px;
    bottom: 8px;
    width: 5px;
  }

  .north { top: 0; cursor: n-resize; }
  .south { bottom: 0; cursor: s-resize; }
  .east { right: 0; cursor: e-resize; }
  .west { left: 0; cursor: w-resize; }

  .northeast,
  .northwest,
  .southeast,
  .southwest {
    width: 8px;
    height: 8px;
  }

  .northeast { top: 0; right: 0; cursor: ne-resize; }
  .northwest { top: 0; left: 0; cursor: nw-resize; }
  .southeast { bottom: 0; right: 0; cursor: se-resize; }
  .southwest { bottom: 0; left: 0; cursor: sw-resize; }
</style>
