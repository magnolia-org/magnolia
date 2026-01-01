<script>
  import { onMount } from 'svelte';
  import { getVersion } from '@tauri-apps/api/app';
  import { invoke } from '@tauri-apps/api/core';

  let currentVersion = '';
  let latestVersion = '';
  let updateAvailable = false;
  let updateUrl = '';
  let downloading = false;
  let installing = false;
  let checkForUpdatesEnabled = true;

  async function checkForUpdates() {
    if (!checkForUpdatesEnabled) {
      console.log('update check disabled in settings');
      return;
    }

    try {
      currentVersion = await getVersion();
      console.log(`current version: ${currentVersion}`);
      
      const response = await fetch('https://api.github.com/repos/chwair/magnolia/releases/latest');
      const release = await response.json();
      
      latestVersion = release.tag_name.replace('v', '');
      console.log(`latest version: ${latestVersion}`);
      
      if (latestVersion !== currentVersion) {
        const asset = release.assets.find(a => a.name.endsWith('.exe'));
        if (asset) {
          updateAvailable = true;
          updateUrl = asset.browser_download_url;
          console.log(`update available: ${latestVersion}`);
        }
      } else {
        console.log('app is up to date');
      }
    } catch (error) {
      console.error('failed to check for updates:', error);
    }
  }

  async function downloadAndInstall() {
    if (downloading || installing) return;
    
    try {
      downloading = true;
      console.log('downloading update...');
      
      const installerPath = await invoke('download_update', { url: updateUrl });
      console.log(`downloaded to: ${installerPath}`);
      
      downloading = false;
      installing = true;
      console.log('installing update...');
      
      await invoke('install_update', { installerPath });
    } catch (error) {
      console.error('update failed:', error);
      downloading = false;
      installing = false;
    }
  }

  function remindLater() {
    updateAvailable = false;
  }

  async function loadSettings() {
    try {
      const settings = await invoke('get_settings');
      checkForUpdatesEnabled = settings.check_for_updates !== undefined ? settings.check_for_updates : true;
    } catch (error) {
      console.error('failed to load settings:', error);
      checkForUpdatesEnabled = true;
    }
  }

  onMount(async () => {
    await loadSettings();
    
    if (checkForUpdatesEnabled) {
      setTimeout(checkForUpdates, 1000);
    }
    
    window.addEventListener('manual-update-check', checkForUpdates);
    window.addEventListener('settingsChanged', async (e) => {
      if (e.detail && e.detail.check_for_updates !== undefined) {
        checkForUpdatesEnabled = e.detail.check_for_updates;
      }
    });
    
    return () => {
      window.removeEventListener('manual-update-check', checkForUpdates);
    };
  });
</script>

{#if updateAvailable}
  <div class="update-notification">
    {#if downloading}
      <div class="notification-content">
        <i class="ri-loader-4-line spin icon"></i>
        <div class="text-content">
          <div class="title">Downloading Update</div>
          <div class="subtitle">Please wait...</div>
        </div>
      </div>
    {:else if installing}
      <div class="notification-content">
        <i class="ri-loader-4-line spin icon"></i>
        <div class="text-content">
          <div class="title">Installing Update</div>
          <div class="subtitle">App will restart soon</div>
        </div>
      </div>
    {:else}
      <div class="notification-content">
        <i class="ri-download-cloud-line icon"></i>
        <div class="text-content">
          <div class="title">Update Available</div>
          <div class="subtitle">Version {latestVersion} is ready</div>
        </div>
      </div>
      
      <div class="button-group">
        <button class="btn btn-secondary" on:click={remindLater}>
          Later
        </button>
        <button class="btn btn-primary" on:click={downloadAndInstall}>
          Install
        </button>
      </div>
    {/if}
  </div>
{/if}

<style>
  .update-notification {
    position: fixed;
    top: 70px;
    right: 20px;
    background: rgba(10, 10, 10, 0.95);
    backdrop-filter: blur(12px);
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: var(--border-radius-lg);
    padding: 16px;
    min-width: 320px;
    max-width: 380px;
    z-index: 10000;
    box-shadow: var(--shadow-depth), 0 8px 32px rgba(0, 0, 0, 0.4);
    animation: slideInRight 0.4s cubic-bezier(0.16, 1, 0.3, 1);
  }

  @keyframes slideInRight {
    from {
      transform: translateX(120%);
      opacity: 0;
    }
    to {
      transform: translateX(0);
      opacity: 1;
    }
  }

  .notification-content {
    display: flex;
    align-items: center;
    gap: 14px;
    margin-bottom: 12px;
  }

  .icon {
    font-size: 32px;
    color: var(--accent-color);
    flex-shrink: 0;
  }

  .text-content {
    flex: 1;
    min-width: 0;
  }

  .title {
    font-size: 15px;
    font-weight: 600;
    color: var(--text-primary);
    margin-bottom: 2px;
  }

  .subtitle {
    font-size: 13px;
    color: var(--text-secondary);
  }

  .button-group {
    display: flex;
    gap: 8px;
    margin-top: 12px;
  }

  .btn {
    flex: 1;
    padding: 10px 16px;
    border: none;
    border-radius: 7px;
    font-size: 14px;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.2s;
    font-family: inherit;
  }

  .btn-primary {
    background: var(--accent-color);
    color: white;
    box-shadow: 0 2px 8px color-mix(in srgb, var(--accent-color) 30%, transparent);
    font-weight: 600;
  }

  .btn-primary:hover {
    opacity: 0.9;
    transform: translateY(-1px);
    box-shadow: 0 4px 12px color-mix(in srgb, var(--accent-color) 40%, transparent);
  }

  .btn-primary:active {
    transform: translateY(0);
  }

  .btn-secondary {
    background: rgba(255, 255, 255, 0.05);
    color: var(--text-secondary);
    border: 1px solid rgba(255, 255, 255, 0.1);
  }

  .btn-secondary:hover {
    background: rgba(255, 255, 255, 0.08);
    color: var(--text-primary);
    border-color: rgba(255, 255, 255, 0.15);
  }

  .spin {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    from {
      transform: rotate(0deg);
    }
    to {
      transform: rotate(360deg);
    }
  }
</style>
