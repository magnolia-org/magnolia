<script>
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import SearchBar from "./SearchBar.svelte";
  import SettingsPanel from "./SettingsPanel.svelte";

  export let searchActive = false;
  export let settingsActive = false;
  export let accentColor = null;
  export let immersive = false;

  const isMacOS = navigator.userAgent.includes("Mac");
  const appWindow = getCurrentWindow();

  async function minimizeWindow() {
    await appWindow.minimize();
  }

  async function maximizeWindow() {
    if (isMacOS) {
      // On macOS, use fullscreen instead of maximize
      const isFullscreen = await appWindow.isFullscreen();
      await appWindow.setFullscreen(!isFullscreen);
    } else {
      await appWindow.toggleMaximize();
    }
  }

  async function closeWindow() {
    await appWindow.close();
  }

  function hexToRgb(hex) {
    if (!hex) return "255, 255, 255";
    hex = hex.replace("#", "");
    const r = parseInt(hex.substring(0, 2), 16);
    const g = parseInt(hex.substring(2, 4), 16);
    const b = parseInt(hex.substring(4, 6), 16);
    return `${r}, ${g}, ${b}`;
  }
</script>

<div
  class="titlebar"
  class:immersive-mode={immersive}
  data-tauri-drag-region
  style={accentColor
    ? `--dynamic-accent: ${accentColor}; background: linear-gradient(135deg, rgba(${hexToRgb(accentColor)}, 0.35), rgba(10, 10, 10, 0.95));`
    : ""}
>
  {#if isMacOS}
    <div class="titlebar-controls macos window-controls">
      <button
        class="titlebar-button close"
        on:click={closeWindow}
        aria-label="Close"
      >
        <div class="dot"></div>
      </button>
      <button
        class="titlebar-button minimize"
        on:click={minimizeWindow}
        aria-label="Minimize"
      >
        <div class="dot"></div>
      </button>
      <button
        class="titlebar-button maximize"
        on:click={maximizeWindow}
        aria-label="Maximize"
      >
        <div class="dot"></div>
      </button>
    </div>
    <div class="titlebar-center">
      <div class="logo"></div>
      {#if !immersive}
        <SearchBar bind:searchActive />
      {/if}
    </div>
    <div class="titlebar-right">
      {#if !immersive}
        <SettingsPanel bind:settingsActive />
      {/if}
    </div>
  {:else}
    <div class="titlebar-left">
      <div class="logo"></div>
      {#if !immersive}
        <SearchBar bind:searchActive />
      {/if}
    </div>
    <div class="titlebar-right">
      {#if !immersive}
        <SettingsPanel bind:settingsActive />
      {/if}
    </div>
    <div class="titlebar-controls windows window-controls">
      <button
        class="titlebar-button minimize"
        on:click={minimizeWindow}
        aria-label="Minimize"
      >
        <i class="ri-subtract-line"></i>
      </button>
      <button
        class="titlebar-button maximize"
        on:click={maximizeWindow}
        aria-label="Maximize"
      >
        <i class="ri-stop-line"></i>
      </button>
      <button
        class="titlebar-button close"
        on:click={closeWindow}
        aria-label="Close"
      >
        <i class="ri-close-line"></i>
      </button>
    </div>
  {/if}
</div>
