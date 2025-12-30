<script>
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import logo from "../media/magnolia.png";

  export let visible = false;
  let installing = false;
  let completed = false;
  let fadingOut = false;
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
      console.log("FFmpeg install progress:", event.payload);
      progress = event.payload;
    });

    return () => {
      unlisten();
    };
  });

  async function installFFmpeg() {
    installing = true;
    completed = false;
    progress = -1; // Start as indeterminate
    error = null;
    try {
      await invoke("install_ffmpeg");
      installing = false;
      completed = true;
      // Show success message for 1.0s then fade out
      setTimeout(() => {
        fadingOut = true;
        // Wait for fade out animation
        setTimeout(() => {
          visible = false;
          completed = false;
          fadingOut = false;
        }, 300);
      }, 1000);
    } catch (err) {
      error = "Failed to install FFmpeg: " + err;
      installing = false;
    }
  }
</script>

{#if visible}
  <div class="onboarding-overlay" class:fading-out={fadingOut}>
    <div class="onboarding-modal">
      <img src={logo} alt="Magnolia Logo" class="logo" />
      <h2>Welcome to Magnolia!</h2>
      <p>
        To play videos, Magnolia requires <strong>FFmpeg</strong>. It seems to be missing from your system.
      </p>
      
      {#if completed}
        <div class="completion-container">
          <div class="success-icon">
            <i class="ri-check-line"></i>
          </div>
          <p class="status-text success">FFmpeg installed successfully!</p>
        </div>
      {:else if error}
        <div class="error-message">
          <i class="ri-error-warning-line"></i>
          {error}
        </div>
        <div class="actions">
          <button class="btn-primary" on:click={installFFmpeg}>
            Try Again
          </button>
        </div>
      {:else if installing}
        <div class="progress-container">
          <div class="progress-bar" class:indeterminate={progress < 0 || progress >= 100}>
            {#if progress >= 0 && progress < 100}
              <div class="progress-fill" style="width: {progress}%"></div>
            {:else}
              <div class="progress-fill-indeterminate"></div>
            {/if}
          </div>
          {#if progress >= 0 && progress < 100}
            <span class="progress-text">{Math.round(progress)}%</span>
          {/if}
        </div>
        <p class="status-text">
          {#if progress >= 100}
            Extracting FFmpeg...
          {:else}
            Downloading FFmpeg...
          {/if}
        </p>
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
    top: var(--titlebar-height, 50px);
    left: 0;
    right: 0;
    bottom: 0;
    background: rgba(0, 0, 0, 0.95);
    backdrop-filter: blur(20px);
    z-index: 9999;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: opacity 0.3s ease, backdrop-filter 0.3s ease;
  }

  .onboarding-overlay.fading-out {
    opacity: 0;
    backdrop-filter: blur(0px);
    pointer-events: none;
  }

  .onboarding-modal {
    background: var(--bg-primary, #0a0a0a);
    padding: 60px;
    border-radius: var(--border-radius-xl, 24px);
    border: 1px solid rgba(255, 255, 255, 0.1);
    max-width: 500px;
    width: 90%;
    text-align: center;
    box-shadow: 0 20px 50px rgba(0,0,0,0.5);
    display: flex;
    flex-direction: column;
    align-items: center;
  }

  .logo {
    width: 120px;
    height: 120px;
    margin-bottom: 24px;
    object-fit: contain;
    filter: drop-shadow(0 0 20px rgba(211, 118, 195, 0.3));
  }

  h2 {
    margin-top: 0;
    margin-bottom: 16px;
    font-size: 28px;
    font-weight: 700;
    color: var(--text-primary, #fff);
    letter-spacing: -0.5px;
  }

  p {
    color: var(--text-secondary, rgba(255, 255, 255, 0.7));
    line-height: 1.6;
    margin-bottom: 32px;
    font-size: 16px;
  }

  .btn-primary {
    background: rgba(255, 255, 255, 0.12);
    color: #fff;
    border: 1px solid rgba(255, 255, 255, 0.2);
    padding: 14px 32px;
    border-radius: var(--border-radius-md, 12px);
    font-size: 16px;
    font-weight: 600;
    cursor: pointer;
    transition: all 0.2s ease;
    backdrop-filter: blur(10px);
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.2);
  }

  .btn-primary:hover {
    background: rgba(255, 255, 255, 0.2);
    border-color: var(--accent-color, #d376c3);
    transform: translateY(-2px);
    box-shadow: 0 8px 20px rgba(211, 118, 195, 0.25);
  }

  .progress-container {
    display: flex;
    align-items: center;
    gap: 15px;
    margin-bottom: 12px;
    width: 100%;
  }

  .progress-bar {
    flex: 1;
    height: 6px;
    background: rgba(255, 255, 255, 0.1);
    border-radius: 3px;
    overflow: hidden;
    position: relative;
  }

  .progress-bar.indeterminate {
    background: rgba(255, 255, 255, 0.06);
  }

  .progress-fill {
    height: 100%;
    background: var(--accent-color, #d376c3);
    transition: width 0.2s;
    border-radius: 3px;
  }

  .progress-fill-indeterminate {
    position: absolute;
    top: 0;
    left: 0;
    height: 100%;
    width: 40%;
    background: linear-gradient(90deg, transparent, var(--accent-color), transparent);
    border-radius: 3px;
    animation: indeterminate 1.5s ease-in-out infinite;
  }

  @keyframes indeterminate {
    0% { left: -40%; }
    100% { left: 100%; }
  }

  .progress-text {
    font-family: monospace;
    color: var(--text-secondary, rgba(255, 255, 255, 0.5));
    font-size: 14px;
    min-width: 40px;
    text-align: right;
  }

  .status-text {
    font-size: 14px;
    color: var(--text-secondary, rgba(255, 255, 255, 0.5));
    margin-bottom: 0;
  }
  
  .status-text.success {
    color: #4ade80;
    font-weight: 500;
    margin-top: 10px;
  }

  .completion-container {
    display: flex;
    flex-direction: column;
    align-items: center;
    animation: fadeIn 0.3s ease;
  }

  .success-icon {
    width: 48px;
    height: 48px;
    border-radius: 50%;
    background: rgba(74, 222, 128, 0.1);
    color: #4ade80;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 24px;
    margin-bottom: 8px;
  }

  @keyframes fadeIn {
    from { opacity: 0; transform: translateY(10px); }
    to { opacity: 1; transform: translateY(0); }
  }
  
  .error-message {
    background: rgba(255, 80, 80, 0.1);
    border: 1px solid rgba(255, 80, 80, 0.3);
    color: #ff5050;
    padding: 12px;
    border-radius: var(--border-radius-sm, 8px);
    margin-bottom: 20px;
    display: flex;
    align-items: center;
    gap: 10px;
    justify-content: center;
    width: 100%;
  }
</style>
