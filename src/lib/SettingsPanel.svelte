<script>
  import { createEventDispatcher, onMount, onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { open } from '@tauri-apps/plugin-dialog';
  import { openModal } from './stores/modalStore.js';
  
  export let settingsActive = false;
  
  const dispatch = createEventDispatcher();
  
  let externalPlayer = 'vlc';
  let externalPlayerCustomPath = '';
  let rememberPreferences = true;
  let showSkipPrompts = true;
  let hideRecommendations = false;
  let checkForUpdates = true;
  let subtitleLanguage = '';
  let streamingClient = 'builtin';
  let debridExtensions = [];
  let settingsPanel;
  let playerDropdownOpen = false;
  let langDropdownOpen = false;
  let clientDropdownOpen = false;
  let settingsLoaded = false;
  
  const playerOptions = [
    { value: 'mpv', label: 'MPV' },
    { value: 'vlc', label: 'VLC' },
    { value: 'custom', label: 'Custom' }
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
  
  async function loadDebridExtensions() {
    try {
      const exts = await invoke('list_extensions');
      debridExtensions = exts.filter(e => e.manifest.type === 'debrid' && e.enabled);
      // If the selected client is no longer available, fall back to built-in.
      if (streamingClient !== 'builtin' && !debridExtensions.some(e => e.id === streamingClient)) {
        streamingClient = 'builtin';
      }
    } catch (e) {
      console.error('failed to load debrid extensions:', e);
    }
  }

  onMount(async () => {
    try {
      const settings = await invoke('get_settings');
      externalPlayer = settings.external_player;
      externalPlayerCustomPath = settings.external_player_custom_path || '';
      rememberPreferences = settings.remember_preferences;
      showSkipPrompts = settings.show_skip_prompts;
      hideRecommendations = settings.hide_recommendations;
      checkForUpdates = settings.check_for_updates !== undefined ? settings.check_for_updates : true;
      subtitleLanguage = settings.subtitle_language || '';
      streamingClient = settings.streaming_client || 'builtin';
      console.log('loaded settings from backend:', settings);
      await loadDebridExtensions();
      // Set loaded flag after a tick to ensure reactive statements see the loaded values
      await new Promise(resolve => setTimeout(resolve, 0));
      settingsLoaded = true;
    } catch (error) {
      console.error('failed to load settings:', error);
      settingsLoaded = true;
    }

    window.addEventListener('extensionsChanged', loadDebridExtensions);
  });

  onDestroy(() => {
    window.removeEventListener('extensionsChanged', loadDebridExtensions);
  });
  
  async function saveSettings() {
    if (!settingsLoaded) return;
    
    try {
      const settings = {
        external_player: externalPlayer,
        external_player_custom_path: externalPlayerCustomPath,
        remember_preferences: rememberPreferences,
        show_skip_prompts: showSkipPrompts,
        hide_recommendations: hideRecommendations,
        check_for_updates: checkForUpdates,
        subtitle_language: subtitleLanguage,
        streaming_client: streamingClient
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
    externalPlayer, externalPlayerCustomPath, rememberPreferences, showSkipPrompts, hideRecommendations, checkForUpdates, subtitleLanguage, streamingClient;
    saveSettings();
  }

  $: selectedLangOption = langOptions.find(o => o.value === subtitleLanguage) ?? langOptions[0];
  $: streamingClientOptions = [
    { value: 'builtin', label: 'Built-in' },
    ...debridExtensions.map(e => ({ value: e.id, label: e.manifest.debrid_name || e.manifest.name })),
  ];
  $: selectedClientOption = streamingClientOptions.find(o => o.value === streamingClient) ?? streamingClientOptions[0];
  
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
        clientDropdownOpen = false;
      }
    }
  }

  function closeSettings() {
    settingsActive = false;
    playerDropdownOpen = false;
    langDropdownOpen = false;
    clientDropdownOpen = false;
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

  async function chooseCustomProgram() {
    try {
      const selected = await open({
        multiple: false,
        directory: false,
        title: 'Select a video player program',
      });
      if (typeof selected === 'string') {
        externalPlayerCustomPath = selected;
      }
    } catch (e) {
      console.error('failed to pick custom player:', e);
    }
  }

  $: customProgramName = externalPlayerCustomPath
    ? externalPlayerCustomPath.split(/[\\/]/).pop()
    : '';

  function toggleLangDropdown(event) {
    event.stopPropagation();
    langDropdownOpen = !langDropdownOpen;
    playerDropdownOpen = false;
  }

  function selectLang(value) {
    subtitleLanguage = value;
    langDropdownOpen = false;
  }

  function toggleClientDropdown(event) {
    event.stopPropagation();
    clientDropdownOpen = !clientDropdownOpen;
    playerDropdownOpen = false;
    langDropdownOpen = false;
  }

  function selectClient(value) {
    streamingClient = value;
    clientDropdownOpen = false;
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

        {#if externalPlayer === 'custom'}
          <div class="setting-item">
            <div class="setting-label">
              <span>Custom player</span>
              {#if customProgramName}
                <span class="setting-hint">{customProgramName}</span>
              {/if}
            </div>
            <div class="setting-control">
              <button class="btn-standard" on:click={chooseCustomProgram} type="button">
                <i class="ri-folder-open-line"></i>
                {externalPlayerCustomPath ? 'Change' : 'Browse'}
              </button>
            </div>
          </div>
        {/if}

        <div class="setting-item">
          <div class="setting-label">
            <span>Streaming client</span>
          </div>
          <div class="setting-control">
            <div class="custom-dropdown">
              <button
                class="dropdown-button"
                on:click={toggleClientDropdown}
                type="button"
              >
                <span>{selectedClientOption?.label ?? 'Built-in'}</span>
                <i class="ri-arrow-down-s-line dropdown-icon" class:open={clientDropdownOpen}></i>
              </button>
              {#if clientDropdownOpen}
                <div class="dropdown-menu">
                  {#each streamingClientOptions as option}
                    <button
                      class="dropdown-option"
                      class:selected={option.value === streamingClient}
                      on:click|stopPropagation={() => selectClient(option.value)}
                      type="button"
                    >
                      {option.label}
                      {#if option.value === streamingClient}
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
            <button class="btn-standard" on:click={() => { openModal('extensions'); closeSettings(); }} type="button">
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
  .setting-hint {
    display: block;
    font-size: 11px;
    color: rgba(255, 255, 255, 0.35);
    margin-top: 3px;
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
