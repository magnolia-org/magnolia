<script>
  import { createEventDispatcher, onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  
  export let settingsActive = false;
  
  const dispatch = createEventDispatcher();
  
  let externalPlayer = 'mpv';
  let rememberPreferences = true;
  let showSkipPrompts = true;
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
      await invoke('save_settings', {
        settings: {
          external_player: externalPlayer,
          remember_preferences: rememberPreferences,
          show_skip_prompts: showSkipPrompts
        }
      });
      console.log('settings saved to backend');
    } catch (error) {
      console.error('failed to save settings:', error);
    }
  }
  
  // Auto-save when any setting changes (tracks the actual variables)
  $: if (settingsLoaded) {
    // This will re-run whenever externalPlayer, rememberPreferences, or showSkipPrompts change
    externalPlayer, rememberPreferences, showSkipPrompts;
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
      <div class="settings-header">
        <h3>Settings</h3>
      </div>
      
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
      </div>
    </div>
  {/if}
</div>
