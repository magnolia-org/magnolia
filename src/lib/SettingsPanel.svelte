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
  let checkForUpdates = true;
  let subtitleLanguage = '';
  let settingsPanel;
  let playerDropdownOpen = false;
  let langDropdownOpen = false;
  let settingsLoaded = false;
  
  const playerOptions = [
    { value: 'mpv', label: 'MPV' },
    { value: 'vlc', label: 'VLC' }
  ];

  const langOptions = [
    { value: '', label: 'Disabled', flag: null },
    { value: 'en', label: 'English', flag: 'GB' },
    { value: 'es', label: 'Spanish', flag: 'ES' },
    { value: 'fr', label: 'French', flag: 'FR' },
    { value: 'de', label: 'German', flag: 'DE' },
    { value: 'it', label: 'Italian', flag: 'IT' },
    { value: 'pt', label: 'Portuguese', flag: 'PT' },
    { value: 'ru', label: 'Russian', flag: 'RU' },
    { value: 'nl', label: 'Dutch', flag: 'NL' },
    { value: 'pl', label: 'Polish', flag: 'PL' },
    { value: 'sv', label: 'Swedish', flag: 'SE' },
    { value: 'tr', label: 'Turkish', flag: 'TR' },
    { value: 'ar', label: 'Arabic', flag: 'SA' },
    { value: 'ja', label: 'Japanese', flag: 'JP' },
    { value: 'ko', label: 'Korean', flag: 'KR' },
    { value: 'zh', label: 'Chinese', flag: 'CN' },
  ];
  
  onMount(async () => {
    try {
      const settings = await invoke('get_settings');
      externalPlayer = settings.external_player;
      rememberPreferences = settings.remember_preferences;
      showSkipPrompts = settings.show_skip_prompts;
      hideRecommendations = settings.hide_recommendations;
      checkForUpdates = settings.check_for_updates !== undefined ? settings.check_for_updates : true;
      subtitleLanguage = settings.subtitle_language || '';
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
        check_for_updates: checkForUpdates,
        subtitle_language: subtitleLanguage
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
    externalPlayer, rememberPreferences, showSkipPrompts, hideRecommendations, checkForUpdates, subtitleLanguage;
    saveSettings();
  }

  $: selectedLangOption = langOptions.find(o => o.value === subtitleLanguage) ?? langOptions[0];
  
  function handleClickOutside(event) {
    if (settingsPanel && !settingsPanel.contains(event.target) && settingsActive) {
      closeSettings();
      return;
    }
    // Close dropdowns if clicking outside them
    const dropdowns = settingsPanel?.querySelectorAll('.custom-dropdown') ?? [];
    for (const dropdown of dropdowns) {
      if (!dropdown.contains(event.target)) {
        playerDropdownOpen = false;
        langDropdownOpen = false;
      }
    }
  }
  
  function closeSettings() {
    settingsActive = false;
    playerDropdownOpen = false;
    langDropdownOpen = false;
    dispatch('close');
  }
  
  function toggleSettings() {
    settingsActive = !settingsActive;
  }
  
  function togglePlayerDropdown(event) {
    event.stopPropagation();
    playerDropdownOpen = !playerDropdownOpen;
    langDropdownOpen = false;
  }
  
  function selectPlayer(value) {
    externalPlayer = value;
    playerDropdownOpen = false;
  }

  function toggleLangDropdown(event) {
    event.stopPropagation();
    langDropdownOpen = !langDropdownOpen;
    playerDropdownOpen = false;
  }

  function selectLang(value) {
    subtitleLanguage = value;
    langDropdownOpen = false;
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
            <span>Auto-select subtitle language</span>
          </div>
          <div class="setting-control">
            <div class="custom-dropdown">
              <button
                class="dropdown-button"
                on:click={toggleLangDropdown}
                type="button"
              >
                {#if selectedLangOption?.flag}
                  <img
                    src="https://flagsapi.com/{selectedLangOption.flag}/flat/64.png"
                    alt={selectedLangOption.label}
                    class="dropdown-flag"
                  />
                {/if}
                <span>{selectedLangOption?.label ?? 'Disabled'}</span>
                <i class="ri-arrow-down-s-line dropdown-icon" class:open={langDropdownOpen}></i>
              </button>
              {#if langDropdownOpen}
                <div class="dropdown-menu lang-dropdown-menu">
                  {#each langOptions as option}
                    <button
                      class="dropdown-option"
                      class:selected={option.value === subtitleLanguage}
                      on:click|stopPropagation={() => selectLang(option.value)}
                      type="button"
                    >
                      <span class="lang-option-left">
                        {#if option.flag}
                          <img
                            src="https://flagsapi.com/{option.flag}/flat/64.png"
                            alt={option.label}
                            class="dropdown-flag"
                          />
                        {:else}
                          <i class="ri-subtract-line dropdown-flag-placeholder"></i>
                        {/if}
                        {option.label}
                      </span>
                      {#if option.value === subtitleLanguage}
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
            <span>Extensions</span>
          </div>
          <div class="setting-control">
            <button class="manage-btn" on:click={() => { openModal('extensions'); closeSettings(); }} type="button">
              <i class="ri-puzzle-line"></i>
              Manage
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
  .manage-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    background: rgba(255, 255, 255, 0.08);
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: 8px;
    color: #fff;
    padding: 6px 12px;
    font-size: 12.5px;
    cursor: pointer;
    transition: background 0.2s;
  }

  .manage-btn:hover {
    background: rgba(255, 255, 255, 0.15);
  }

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

  .dropdown-flag {
    width: 16px;
    height: 12px;
    object-fit: cover;
    border-radius: 2px;
    flex-shrink: 0;
  }

  .dropdown-flag-placeholder {
    width: 16px;
    text-align: center;
    color: rgba(255, 255, 255, 0.25);
    flex-shrink: 0;
  }

  .lang-dropdown-menu {
    max-height: 220px;
    overflow-y: auto;
  }

  .lang-option-left {
    display: flex;
    align-items: center;
    gap: 8px;
  }
</style>
