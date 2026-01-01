<script>
  import { createEventDispatcher, onMount } from 'svelte';
  import { fade, scale } from 'svelte/transition';
  import { getVersion } from '@tauri-apps/api/app';

  const dispatch = createEventDispatcher();
  let version = '';

  onMount(async () => {
    try {
      version = await getVersion();
    } catch (e) {
      console.error('failed to get version', e);
      version = 'Unknown';
    }
  });

  async function openGitHub() {
    try {
      const { open } = await import('@tauri-apps/plugin-shell');
      await open('https://github.com/chwair/magnolia');
    } catch (e) {
      console.error('failed to open github', e);
    }
  }

  function checkForUpdates() {
    window.dispatchEvent(new CustomEvent('manual-update-check'));
    dispatch('close');
  }
</script>

<div class="modal-overlay" on:click={() => dispatch('close')} transition:fade>
  <div class="modal-content" on:click|stopPropagation transition:scale>
    <div class="about-header">
      <div class="logo"></div>
      <h1>Magnolia</h1>
      <span class="version">v{version}</span>
    </div>
    
    <div class="about-body">
      <div class="links">
        <button class="link-btn" on:click={openGitHub}>
          <i class="ri-github-fill"></i> GitHub
        </button>
        <button class="link-btn" on:click={checkForUpdates}>
          <i class="ri-refresh-line"></i> Check for Updates
        </button>
      </div>
    </div>
  </div>
</div>

<style>
  .modal-overlay {
    position: fixed;
    top: 0;
    left: 0;
    width: 100%;
    height: 100%;
    background: rgba(0, 0, 0, 0.85);
    display: flex;
    justify-content: center;
    align-items: center;
    z-index: 9750;
    backdrop-filter: blur(8px);
  }

  .modal-content {
    background: var(--bg-primary);
    width: 92%;
    border-radius: var(--border-radius-lg);
    border: 1px solid rgba(255, 255, 255, 0.08);
    display: flex;
    flex-direction: column;
    box-shadow: var(--shadow-depth), 0 20px 60px rgba(0, 0, 0, 0.6);
    overflow: hidden;
  }

  .modal-content {
    max-width: 400px;
    padding: 40px;
    align-items: center;
    text-align: center;
  }

  .about-header {
    display: flex;
    flex-direction: column;
    align-items: center;
    margin-bottom: 24px;
  }

  .logo {
    width: 80px;
    height: 80px;
    background-image: url("/src/media/magnolia.png");
    background-repeat: no-repeat;
    background-size: contain;
    background-position: center;
    margin-bottom: 16px;
  }

  h1 {
    font-size: 24px;
    font-weight: 700;
    margin: 0 0 8px 0;
    color: var(--text-primary);
  }

  .version {
    font-size: 14px;
    color: var(--text-secondary);
    background: rgba(255, 255, 255, 0.1);
    padding: 4px 12px;
    border-radius: 12px;
  }

  .links {
    display: flex;
    justify-content: center;
    gap: 16px;
  }

  .link-btn {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 20px;
    background: var(--bg-tertiary);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: var(--border-radius-sm);
    color: var(--text-secondary);
    font-size: 13px;
    font-weight: 500;
    font-family: inherit;
    cursor: pointer;
    transition: all 0.2s ease;
  }

  .link-btn:hover {
    background: rgba(255, 255, 255, 0.08);
    border-color: rgba(255, 255, 255, 0.15);
    color: var(--text-primary);
  }

  .link-btn i {
    font-size: 20px;
  }
</style>
