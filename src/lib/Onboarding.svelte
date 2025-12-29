<script>
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";

  let visible = false;
  let installing = false;
  let progress = 0;
  let error = null;

  onMount(async () => {
    try {
      const installed = await invoke("check_ffmpeg");
      if (!installed) {
        visible = true;
      }
    } catch (err) {
      console.error("Failed to check ffmpeg:", err);
    }

    const unlisten = await listen("ffmpeg-install-progress", (event) => {
      progress = event.payload;
    });

    return () => {
      unlisten();
    };
  });

  async function installFFmpeg() {
    installing = true;
    error = null;
    try {
      await invoke("install_ffmpeg");
      visible = false;
      // Reload or just continue?
      // Maybe show success message briefly
    } catch (err) {
      error = "Failed to install FFmpeg: " + err;
      installing = false;
    }
  }
</script>

{#if visible}
  <div class="onboarding-overlay">
    <div class="onboarding-modal">
      <h2>Welcome to Magnolia</h2>
      <p>
        To play videos, Magnolia requires <strong>FFmpeg</strong>. It seems to be missing from your system.
      </p>
      
      {#if error}
        <div class="error-message">
          <i class="ri-error-warning-line"></i>
          {error}
        </div>
      {/if}

      {#if installing}
        <div class="progress-container">
          <div class="progress-bar">
            <div class="progress-fill" style="width: {progress}%"></div>
          </div>
          <span class="progress-text">{Math.round(progress)}%</span>
        </div>
        <p class="status-text">Downloading and installing FFmpeg...</p>
      {:else}
        <div class="actions">
          <button class="btn-primary" on:click={installFFmpeg}>
            Install FFmpeg
          </button>
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .onboarding-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.9);
    backdrop-filter: blur(10px);
    z-index: 99999;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .onboarding-modal {
    background: #1a1a1a;
    padding: 40px;
    border-radius: 16px;
    border: 1px solid rgba(255, 255, 255, 0.1);
    max-width: 500px;
    width: 90%;
    text-align: center;
    box-shadow: 0 20px 50px rgba(0,0,0,0.5);
  }

  h2 {
    margin-top: 0;
    margin-bottom: 20px;
    font-size: 24px;
    color: #fff;
  }

  p {
    color: #ccc;
    line-height: 1.6;
    margin-bottom: 30px;
  }

  .btn-primary {
    background: var(--accent-color, #2ed573);
    color: #000;
    border: none;
    padding: 12px 30px;
    border-radius: 8px;
    font-size: 16px;
    font-weight: 600;
    cursor: pointer;
    transition: transform 0.2s;
  }

  .btn-primary:hover {
    transform: scale(1.05);
  }

  .progress-container {
    display: flex;
    align-items: center;
    gap: 15px;
    margin-bottom: 10px;
  }

  .progress-bar {
    flex: 1;
    height: 8px;
    background: rgba(255, 255, 255, 0.1);
    border-radius: 4px;
    overflow: hidden;
  }

  .progress-fill {
    height: 100%;
    background: var(--accent-color, #2ed573);
    transition: width 0.2s;
  }

  .progress-text {
    font-family: monospace;
    color: #888;
  }

  .status-text {
    font-size: 14px;
    color: #888;
    margin-bottom: 0;
  }
  
  .error-message {
    background: rgba(255, 80, 80, 0.1);
    border: 1px solid rgba(255, 80, 80, 0.3);
    color: #ff5050;
    padding: 12px;
    border-radius: 8px;
    margin-bottom: 20px;
    display: flex;
    align-items: center;
    gap: 10px;
    justify-content: center;
  }
</style>
