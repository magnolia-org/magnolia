<script>
  import { createEventDispatcher, onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { openModal } from './stores/modalStore.js';
  
  export let settingsActive = false;
  
  const dispatch = createEventDispatcher();
  
  let externalPlayer = 'vlc';
  let rememberPreferences = true;
  let showSkipPrompts = true;
  let hideRecommendations = false;
  let clearCacheAfterWatch = false;
  let checkForUpdates = true;
  let settingsPanel;
  let playerDropdownOpen = false;
  let settingsLoaded = false;
  
  const playerOptions = [
    { value: 'mpv', label: 'MPV' },
    { value: 'vlc', label: 'VLC' }
  ];
  
  onMount(async () => {
    try {
      const settings = await invoke('get_settings');
      externalPlayer = settings.external_player;
      rememberPreferences = settings.remember_preferences;
      showSkipPrompts = settings.show_skip_prompts;
      hideRecommendations = settings.hide_recommendations;
      clearCacheAfterWatch = settings.clear_cache_after_watch;
      checkForUpdates = settings.check_for_updates !== undefined ? settings.check_for_updates : true;
      console.log('loaded settings from backend:', settings);
      // Set loaded flag after a tick to ensure reactive statements see the loaded values
      await new Promise(resolve => setTimeout(resolve, 0));
      settingsLoaded = true;
    } catch (error) {
      console.error('failed to load settings:', error);
      settingsLoaded = true;
    }
  });
  
  async function saveSettings() {
    if (!settingsLoaded) return;
    
    try {
      const settings = {
        external_player: externalPlayer,
        remember_preferences: rememberPreferences,
        show_skip_prompts: showSkipPrompts,
        hide_recommendations: hideRecommendations,
        clear_cache_after_watch: clearCacheAfterWatch,
        check_for_updates: checkForUpdates
      };
      await invoke('save_settings', { settings });
      console.log('settings saved to backend');
      
      // Dispatch event to notify App.svelte of settings change
      window.dispatchEvent(new CustomEvent('settingsChanged', { detail: settings }));
    } catch (error) {
      console.error('failed to save settings:', error);
    }
  }

  // Auto-save when any setting changes (tracks the actual variables)
  $: if (settingsLoaded) {
    // This will re-run whenever externalPlayer, rememberPreferences, or showSkipPrompts change
    externalPlayer, rememberPreferences, showSkipPrompts, hideRecommendations, clearCacheAfterWatch, checkForUpdates;
    saveSettings();
  }
  
  function handleClickOutside(event) {
    if (settingsPanel && !settingsPanel.contains(event.target) && settingsActive) {
      closeSettings();
    }
    // Close dropdown if clicking outside of it but inside settings panel
    const dropdown = settingsPanel?.querySelector('.custom-dropdown');
    if (dropdown && !dropdown.contains(event.target) && playerDropdownOpen) {
      playerDropdownOpen = false;
    }
  }
  
  function closeSettings() {
    settingsActive = false;
    playerDropdownOpen = false;
    dispatch('close');
  }
  
  function toggleSettings() {
    settingsActive = !settingsActive;
  }
  
  function togglePlayerDropdown(event) {
    event.stopPropagation();
    playerDropdownOpen = !playerDropdownOpen;
  }
  
  function selectPlayer(value) {
    externalPlayer = value;
    playerDropdownOpen = false;
  }
</script>

<svelte:window on:click={handleClickOutside} />

<div class="settings-wrapper" bind:this={settingsPanel}>
  <button 
    class="settings-button titlebar-button" 
    on:click|stopPropagation={toggleSettings}
    aria-label="Settings"
  >
    <i class="ri-settings-3-line"></i>
  </button>
  
  {#if settingsActive}
    <div class="settings-panel">
      <div class="settings-content">
        <div class="setting-item">
          <div class="setting-label">
            <span>External video player</span>
          </div>
          <div class="setting-control">
            <div class="custom-dropdown">
              <button 
                class="dropdown-button" 
                on:click={togglePlayerDropdown}
                type="button"
              >
                <span>{playerOptions.find(o => o.value === externalPlayer)?.label}</span>
                <i class="ri-arrow-down-s-line dropdown-icon" class:open={playerDropdownOpen}></i>
              </button>
              {#if playerDropdownOpen}
                <div class="dropdown-menu">
                  {#each playerOptions as option}
                    <button
                      class="dropdown-option"
                      class:selected={option.value === externalPlayer}
                      on:click|stopPropagation={() => selectPlayer(option.value)}
                      type="button"
                    >
                      {option.label}
                      {#if option.value === externalPlayer}
                        <i class="ri-check-line"></i>
                      {/if}
                    </button>
                  {/each}
                </div>
              {/if}
            </div>
          </div>
        </div>
        
        <div class="setting-item">
          <div class="setting-label">
            <span>Remember audio track & subtitle preference</span>
          </div>
          <div class="setting-control">
            <label class="toggle-switch">
              <input type="checkbox" bind:checked={rememberPreferences} />
              <span class="toggle-slider"></span>
            </label>
          </div>
        </div>
        
        <div class="setting-item">
          <div class="setting-label">
            <span>Show skip intro/outro prompts</span>
          </div>
          <div class="setting-control">
            <label class="toggle-switch">
              <input type="checkbox" bind:checked={showSkipPrompts} />
              <span class="toggle-slider"></span>
            </label>
          </div>
        </div>

        <div class="setting-item">
          <div class="setting-label">
            <span>Hide recommendations</span>
          </div>
          <div class="setting-control">
            <label class="toggle-switch">
              <input type="checkbox" bind:checked={hideRecommendations} />
              <span class="toggle-slider"></span>
            </label>
          </div>
        </div>

        <div class="setting-item">
          <div class="setting-label">
            <span>Clear cache after watch</span>
          </div>
          <div class="setting-control">
            <label class="toggle-switch">
              <input type="checkbox" bind:checked={clearCacheAfterWatch} />
              <span class="toggle-slider"></span>
            </label>
          </div>
        </div>

        <div class="setting-item">
          <div class="setting-label">
            <span>Check for updates on startup</span>
          </div>
          <div class="setting-control">
            <label class="toggle-switch">
              <input type="checkbox" bind:checked={checkForUpdates} />
              <span class="toggle-slider"></span>
            </label>
          </div>
        </div>

        <div class="setting-item">
          <div class="setting-label">
            <span>Storage</span>
          </div>
          <div class="setting-control">
            <button class="btn-standard" on:click={() => { openModal('cache'); closeSettings(); }}>
              Manage Cache
            </button>
          </div>
        </div>
      </div>

      <div class="about-link">
        <button class="btn-link" on:click={() => { openModal('about'); closeSettings(); }}>
          About
        </button>
      </div>
    </div>
  {/if}
</div>

<style>
  .about-link {
    padding: 8px 0;
    display: flex;
    justify-content: center;
    border-top: 1px solid rgba(255, 255, 255, 0.05);
  }

  .btn-link {
    background: transparent;
    border: none;
    color: rgba(255, 255, 255, 0.3);
    font-size: 11px;
    cursor: pointer;
    padding: 4px;
    transition: all 0.2s;
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .btn-link:hover {
    color: rgba(255, 255, 255, 0.6);
  }
</style>
