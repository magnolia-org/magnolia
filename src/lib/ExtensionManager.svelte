<script>
  import { onMount, createEventDispatcher } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { fade, scale, slide } from 'svelte/transition';
  import { cubicOut } from 'svelte/easing';
  import { open as openFileDialog } from '@tauri-apps/plugin-dialog';

  const dispatch = createEventDispatcher();

  let extensions = [];
  let loading = true;
  let installUrl = '';
  let installing = false;
  let installError = '';
  let expandedId = null;
  let fieldDrafts = {};    // ext id -> { key: value }
  let originalDrafts = {}; // ext id -> { key: value } (last saved state)
  let savedId = null;
  let updatingId = null;
  let confirmRemoveId = null;

  $: trackers = extensions.filter(e => e.manifest.type === 'tracker');
  $: subtitleSources = extensions.filter(e => e.manifest.type === 'subtitles');
  $: dirtyIds = new Set(
    Object.keys(fieldDrafts).filter(
      id => JSON.stringify(fieldDrafts[id]) !== JSON.stringify(originalDrafts[id])
    )
  );

  async function refresh() {
    extensions = await invoke('list_extensions');
    // (Re)build field drafts from stored values + manifest defaults
    const drafts = {};
    for (const ext of extensions) {
      const draft = {};
      for (const field of ext.manifest.fields || []) {
        const stored = ext.field_values?.[field.key];
        draft[field.key] = stored !== undefined
          ? stored
          : field.default != null ? String(field.default) : '';
      }
      drafts[ext.id] = draft;
    }
    fieldDrafts = drafts;
    originalDrafts = JSON.parse(JSON.stringify(drafts));
  }

  onMount(async () => {
    try {
      await refresh();
    } catch (e) {
      console.error('failed to load extensions:', e);
    }
    loading = false;
  });

  function notifyChanged() {
    window.dispatchEvent(new CustomEvent('extensionsChanged'));
  }

  async function install(fn) {
    installing = true;
    installError = '';
    try {
      await fn();
      await refresh();
      notifyChanged();
      installUrl = '';
    } catch (e) {
      installError = String(e);
    } finally {
      installing = false;
    }
  }

  async function installFromFile() {
    const selected = await openFileDialog({
      multiple: false,
      filters: [{ name: 'Rhai extension', extensions: ['rhai'] }],
    });
    if (!selected) return;
    const path = typeof selected === 'string' ? selected : selected.path;
    if (!path) return;
    await install(() => invoke('install_extension_from_path', { path }));
  }

  async function installFromUrl() {
    const url = installUrl.trim();
    if (!url) return;
    await install(() => invoke('install_extension_from_url', { url }));
  }

  async function updateFromOrigin(ext) {
    if (updatingId) return;
    updatingId = ext.id;
    installError = '';
    try {
      const url = ext.origin.slice(4); // strip "url:"
      await invoke('install_extension_from_url', { url });
      await refresh();
      notifyChanged();
    } catch (e) {
      installError = `Update of ${ext.manifest.name} failed: ${e}`;
    } finally {
      updatingId = null;
    }
  }

  async function toggleEnabled(ext) {
    try {
      await invoke('set_extension_enabled', { id: ext.id, enabled: !ext.enabled });
      await refresh();
      notifyChanged();
    } catch (e) {
      console.error('failed to toggle extension:', e);
    }
  }

  async function removeExtension(ext) {
    if (confirmRemoveId !== ext.id) {
      confirmRemoveId = ext.id;
      setTimeout(() => { if (confirmRemoveId === ext.id) confirmRemoveId = null; }, 3000);
      return;
    }
    confirmRemoveId = null;
    try {
      await invoke('remove_extension', { id: ext.id });
      if (expandedId === ext.id) expandedId = null;
      await refresh();
      notifyChanged();
    } catch (e) {
      installError = String(e);
    }
  }

  async function saveFields(ext) {
    try {
      await invoke('set_extension_field_values', {
        id: ext.id,
        values: { ...fieldDrafts[ext.id] },
      });
      originalDrafts[ext.id] = JSON.parse(JSON.stringify(fieldDrafts[ext.id]));
      originalDrafts = originalDrafts;
      savedId = ext.id;
      setTimeout(() => { if (savedId === ext.id) savedId = null; }, 1500);
    } catch (e) {
      console.error('failed to save extension fields:', e);
    }
  }

  function toggleExpanded(ext) {
    expandedId = expandedId === ext.id ? null : ext.id;
  }

  function iconSrc(ext) {
    const icon = ext.manifest.icon;
    if (!icon) return null;
    return icon.startsWith('data:') ? icon : `data:image/png;base64,${icon}`;
  }

  function originLabel(ext) {
    if (ext.origin.startsWith('url:')) return ext.origin.slice(4);
    if (ext.origin.startsWith('file:')) return ext.origin.slice(5);
    return ext.origin;
  }

  function openDocs() {
    invoke('open_external_url', {
      url: 'https://github.com/chwair/magnolia/wiki/Extensions',
    }).catch(() => {});
  }
</script>

<div class="modal-overlay" on:click={() => dispatch('close')} transition:fade|global={{ duration: 400, easing: cubicOut }}>
  <div class="modal-content" on:click|stopPropagation in:scale|global={{ start: 1.08, opacity: 1, duration: 400, easing: cubicOut }} out:scale|global={{ start: 1.08, opacity: 1, duration: 400, easing: cubicOut }}>
    <div class="modal-header">
      <div class="header-text">
        <h3>Extensions</h3>
        {#if !loading}
          <span class="header-sub">{extensions.length} installed</span>
        {/if}
      </div>
      <button class="close-btn" on:click={() => dispatch('close')} aria-label="Close">
        <i class="ri-close-line"></i>
      </button>
    </div>

    <div class="modal-body">
      <div class="install-bar">
        <button class="btn-standard" on:click={installFromFile} disabled={installing}>
          <i class="ri-folder-open-line"></i> From file
        </button>
        <div class="url-install">
          <i class="ri-link url-prefix-icon"></i>
          <input
            type="text"
            placeholder="https://example.com/extension.rhai"
            bind:value={installUrl}
            on:keydown={(e) => e.key === 'Enter' && installFromUrl()}
            disabled={installing}
          />
          <button class="btn-standard accent" on:click={installFromUrl} disabled={installing || !installUrl.trim()}>
            {#if installing}
              <span class="spinner"></span>
            {:else}
              <i class="ri-download-cloud-line"></i>
            {/if}
            Install
          </button>
        </div>
      </div>
      {#if installError}
        <div class="install-error" transition:slide={{ duration: 150 }}>
          <i class="ri-error-warning-line"></i>
          <span>{installError}</span>
          <button class="error-dismiss" on:click={() => (installError = '')} aria-label="Dismiss">
            <i class="ri-close-line"></i>
          </button>
        </div>
      {/if}

      {#if loading}
        <div class="empty-state">Loading extensions...</div>
      {:else if extensions.length === 0}
        <div class="empty-state">
          <i class="ri-puzzle-line empty-icon"></i>
          <p>No extensions installed</p>
          <span>Install a <code>.rhai</code> file or paste a URL above.</span>
        </div>
      {:else}
        {#each [
          { label: 'Trackers', icon: 'ri-radar-line', items: trackers },
          { label: 'Subtitle sources', icon: 'ri-closed-captioning-line', items: subtitleSources },
        ] as section}
          {#if section.items.length > 0}
            <div class="section-header">
              <i class={section.icon}></i>
              <span>{section.label}</span>
              <span class="section-count">{section.items.length}</span>
            </div>
            <div class="extension-list">
              {#each section.items as ext (ext.id)}
                <div class="extension-card" class:disabled={!ext.enabled} class:expanded={expandedId === ext.id}>
                  <div class="ext-row">
                    <div class="ext-icon">
                      {#if iconSrc(ext)}
                        <img src={iconSrc(ext)} alt={ext.manifest.name} />
                      {:else}
                        <i class={ext.manifest.type === 'tracker' ? 'ri-radar-line' : 'ri-closed-captioning-line'}></i>
                      {/if}
                    </div>
                    <div class="ext-details">
                      <div class="ext-title-row">
                        <span class="ext-name">{ext.manifest.name}</span>
                        {#if ext.manifest.version}
                          <span class="ext-version">v{ext.manifest.version}</span>
                        {/if}
                        {#if ext.origin === 'preinstalled'}
                          <span class="ext-badge preinstalled-badge">Preinstalled</span>
                        {/if}
                        {#if ext.manifest.type === 'tracker' && ext.manifest.is_anime}
                          <span class="ext-badge anime-badge">Anime</span>
                        {/if}
                        {#if ext.manifest.type === 'subtitles'}
                          <span class="ext-badge auto-badge">{ext.manifest.can_auto_fetch ? 'Auto-fetch' : 'Manual'}</span>
                        {/if}
                      </div>
                      <div class="ext-meta">
                        {#if ext.manifest.author}
                          <span class="ext-author">by {ext.manifest.author}</span>
                        {/if}
                        {#if ext.origin !== 'preinstalled'}
                          {#if ext.manifest.author}<span class="dot">•</span>{/if}
                          <span class="ext-origin" title={originLabel(ext)}>
                            <i class={ext.origin.startsWith('url:') ? 'ri-global-line' : 'ri-file-3-line'}></i>
                            {originLabel(ext)}
                          </span>
                        {/if}
                      </div>
                    </div>
                    <div class="ext-actions">
                      {#if ext.origin.startsWith('url:')}
                        <button
                          class="btn-icon"
                          title="Re-download from source URL"
                          on:click={() => updateFromOrigin(ext)}
                          disabled={updatingId !== null}
                        >
                          {#if updatingId === ext.id}
                            <span class="spinner small"></span>
                          {:else}
                            <i class="ri-refresh-line"></i>
                          {/if}
                        </button>
                      {/if}
                      {#if (ext.manifest.fields || []).length > 0}
                        <button class="btn-icon" class:active-btn={expandedId === ext.id} title="Settings" on:click={() => toggleExpanded(ext)}>
                          <i class="ri-settings-4-line"></i>
                        </button>
                      {/if}
                      {#if ext.origin !== 'preinstalled'}
                        <button
                          class="btn-icon danger"
                          class:confirming={confirmRemoveId === ext.id}
                          title={confirmRemoveId === ext.id ? 'Click again to confirm' : 'Remove'}
                          on:click={() => removeExtension(ext)}
                        >
                          <i class={confirmRemoveId === ext.id ? 'ri-question-line' : 'ri-delete-bin-line'}></i>
                        </button>
                      {/if}
                      <label class="toggle-switch">
                        <input type="checkbox" checked={ext.enabled} on:change={() => toggleEnabled(ext)} />
                        <span class="toggle-slider"></span>
                      </label>
                    </div>
                  </div>

                  {#if expandedId === ext.id && (ext.manifest.fields || []).length > 0}
                    <div class="ext-fields" transition:slide={{ duration: 200 }}>
                      {#each ext.manifest.fields as field (field.key)}
                        <div class="ext-field">
                          <label class="ext-field-label" for="ext-field-{ext.id}-{field.key}">
                            {field.label || field.key}
                            {#if field.hidden}
                              <i class="ri-lock-line field-lock" title="Stored value is hidden"></i>
                            {/if}
                          </label>
                          {#if field.type === 'checkbox'}
                            <label class="toggle-switch small">
                              <input
                                id="ext-field-{ext.id}-{field.key}"
                                type="checkbox"
                                checked={fieldDrafts[ext.id]?.[field.key] === 'true'}
                                on:change={(e) => { fieldDrafts[ext.id][field.key] = e.target.checked ? 'true' : 'false'; }}
                              />
                              <span class="toggle-slider"></span>
                            </label>
                          {:else if field.type === 'select'}
                            <select
                              id="ext-field-{ext.id}-{field.key}"
                              bind:value={fieldDrafts[ext.id][field.key]}
                            >
                              {#each field.options || [] as option}
                                <option value={option}>{option}</option>
                              {/each}
                            </select>
                          {:else if field.type === 'number'}
                            <input
                              id="ext-field-{ext.id}-{field.key}"
                              type="number"
                              placeholder={field.placeholder || ''}
                              bind:value={fieldDrafts[ext.id][field.key]}
                            />
                          {:else}
                            <input
                              id="ext-field-{ext.id}-{field.key}"
                              type={field.hidden ? 'password' : 'text'}
                              placeholder={field.placeholder || ''}
                              bind:value={fieldDrafts[ext.id][field.key]}
                            />
                          {/if}
                        </div>
                      {/each}
                      <div class="ext-fields-footer">
                        {#if savedId === ext.id}
                          <span class="saved-hint" transition:fade={{ duration: 150 }}>
                            <i class="ri-check-line"></i> Saved
                          </span>
                        {/if}
                        <button
                          class="btn-standard accent"
                          on:click={() => saveFields(ext)}
                          disabled={!dirtyIds.has(ext.id)}
                        >
                          <i class="ri-save-line"></i> Save
                        </button>
                      </div>
                    </div>
                  {/if}
                </div>
              {/each}
            </div>
          {/if}
        {/each}
      {/if}
    </div>

    <div class="modal-footer">
      <button class="btn-link" on:click={openDocs}>
        <i class="ri-book-2-line"></i> Extension guide
      </button>
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
    max-width: 680px;
    max-height: 85vh;
    border-radius: var(--border-radius-lg);
    display: flex;
    flex-direction: column;
    overflow: hidden;
    border: 1px solid rgba(255, 255, 255, 0.08);
  }

  .modal-header {
    padding: 18px 22px 14px;
    border-bottom: 1px solid rgba(255, 255, 255, 0.06);
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .header-text {
    display: flex;
    align-items: baseline;
    gap: 10px;
  }

  .modal-header h3 {
    margin: 0;
    font-size: 17px;
    font-weight: 600;
  }

  .header-sub {
    color: rgba(255, 255, 255, 0.35);
    font-size: 12.5px;
  }

  .close-btn {
    background: transparent;
    border: none;
    color: rgba(255, 255, 255, 0.5);
    font-size: 20px;
    cursor: pointer;
    padding: 4px;
    border-radius: 6px;
    display: flex;
    transition: all 0.15s;
  }

  .close-btn:hover {
    background: rgba(255, 255, 255, 0.08);
    color: #fff;
  }

  .modal-body {
    padding: 16px 22px;
    overflow-y: auto;
    flex: 1;
  }

  .modal-footer {
    padding: 10px 22px;
    border-top: 1px solid rgba(255, 255, 255, 0.05);
    display: flex;
    justify-content: center;
  }

  .btn-link {
    background: transparent;
    border: none;
    color: rgba(255, 255, 255, 0.35);
    font-size: 12px;
    cursor: pointer;
    padding: 4px;
    transition: color 0.2s;
    display: flex;
    align-items: center;
    gap: 5px;
  }

  .btn-link:hover {
    color: rgba(255, 255, 255, 0.7);
  }

  .install-bar {
    display: flex;
    gap: 10px;
    flex-wrap: wrap;
    margin-bottom: 12px;
  }

  .url-install {
    display: flex;
    gap: 8px;
    flex: 1;
    min-width: 280px;
    align-items: center;
    position: relative;
  }

  .url-prefix-icon {
    position: absolute;
    left: 11px;
    color: rgba(255, 255, 255, 0.3);
    font-size: 14px;
    pointer-events: none;
  }

  .url-install input {
    flex: 1;
    background: rgba(255, 255, 255, 0.06);
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: 8px;
    color: #fff;
    padding: 8px 12px 8px 32px;
    font-size: 13px;
    outline: none;
    transition: border-color 0.15s;
  }

  .url-install input:focus {
    border-color: var(--accent-color);
  }

  .btn-standard {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    background: rgba(255, 255, 255, 0.08);
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: 8px;
    color: #fff;
    padding: 8px 14px;
    font-size: 13px;
    cursor: pointer;
    transition: background 0.2s;
    white-space: nowrap;
  }

  .btn-standard:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.14);
  }

  .btn-standard.accent {
    background: color-mix(in srgb, var(--accent-color, #9a6ad0) 28%, transparent);
    border-color: color-mix(in srgb, var(--accent-color, #9a6ad0) 45%, transparent);
  }

  .btn-standard.accent:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent-color, #9a6ad0) 42%, transparent);
  }

  .btn-standard:disabled {
    opacity: 0.45;
    cursor: default;
  }

  .install-error {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    color: #ff7878;
    background: rgba(255, 80, 80, 0.08);
    border: 1px solid rgba(255, 80, 80, 0.25);
    border-radius: 8px;
    padding: 8px 12px;
    font-size: 12.5px;
    margin-bottom: 12px;
    word-break: break-word;
  }

  .install-error span {
    flex: 1;
  }

  .error-dismiss {
    background: transparent;
    border: none;
    color: rgba(255, 120, 120, 0.7);
    cursor: pointer;
    padding: 0;
    font-size: 14px;
    display: flex;
  }

  .error-dismiss:hover {
    color: #ff9b9b;
  }

  .section-header {
    display: flex;
    align-items: center;
    gap: 7px;
    margin: 16px 2px 8px;
    color: rgba(255, 255, 255, 0.55);
    font-size: 12px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.6px;
  }

  .section-header i {
    font-size: 14px;
    color: var(--accent-color, #c69ae0);
  }

  .section-count {
    background: rgba(255, 255, 255, 0.08);
    border-radius: 99px;
    padding: 1px 7px;
    font-size: 11px;
    color: rgba(255, 255, 255, 0.5);
  }

  .extension-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .empty-state {
    text-align: center;
    color: rgba(255, 255, 255, 0.4);
    padding: 40px 0;
    font-size: 14px;
    display: flex;
    flex-direction: column;
    gap: 4px;
    align-items: center;
  }

  .empty-icon {
    font-size: 34px;
    color: rgba(255, 255, 255, 0.18);
    margin-bottom: 6px;
  }

  .empty-state p {
    margin: 0;
    font-weight: 500;
    color: rgba(255, 255, 255, 0.6);
  }

  .empty-state span {
    font-size: 12.5px;
  }

  .empty-state code {
    background: rgba(255, 255, 255, 0.08);
    border-radius: 4px;
    padding: 1px 5px;
    font-size: 11.5px;
  }

  .extension-card {
    background: rgba(255, 255, 255, 0.04);
    border: 1px solid rgba(255, 255, 255, 0.07);
    border-radius: 10px;
    transition: border-color 0.15s, background 0.15s;
  }

  .extension-card:hover {
    background: rgba(255, 255, 255, 0.055);
  }

  .extension-card.expanded {
    border-color: rgba(255, 255, 255, 0.14);
  }

  .extension-card.disabled .ext-icon,
  .extension-card.disabled .ext-details {
    opacity: 0.45;
  }

  .ext-row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 11px 14px;
  }

  .ext-icon {
    width: 42px;
    height: 42px;
    flex-shrink: 0;
    border-radius: 9px;
    background: rgba(255, 255, 255, 0.07);
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 20px;
    color: var(--accent-color, #c69ae0);
    overflow: hidden;
    transition: opacity 0.2s;
  }

  .ext-icon img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .ext-details {
    flex: 1;
    min-width: 0;
    transition: opacity 0.2s;
  }

  .ext-title-row {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }

  .ext-name {
    font-weight: 600;
    font-size: 14px;
  }

  .ext-version {
    color: rgba(255, 255, 255, 0.4);
    font-size: 12px;
  }

  .ext-badge {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.4px;
    padding: 2px 7px;
    border-radius: 99px;
  }

  .ext-badge.preinstalled-badge {
    background: rgba(255, 255, 255, 0.09);
    color: rgba(255, 255, 255, 0.55);
  }

  .ext-badge.anime-badge {
    background: rgba(230, 120, 160, 0.2);
    color: #f3b2cb;
  }

  .ext-badge.auto-badge {
    background: rgba(120, 200, 140, 0.18);
    color: #b3e3c0;
  }

  .ext-meta {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 3px;
    color: rgba(255, 255, 255, 0.4);
    font-size: 12px;
    min-width: 0;
  }

  .ext-origin {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 260px;
  }

  .ext-origin i {
    font-size: 12px;
    flex-shrink: 0;
  }

  .ext-actions {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-shrink: 0;
  }

  .btn-icon {
    background: transparent;
    border: none;
    color: rgba(255, 255, 255, 0.55);
    font-size: 16px;
    cursor: pointer;
    padding: 6px;
    border-radius: 6px;
    transition: all 0.15s;
    display: flex;
  }

  .btn-icon:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.08);
    color: #fff;
  }

  .btn-icon:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .btn-icon.active-btn {
    background: rgba(255, 255, 255, 0.1);
    color: #fff;
  }

  .btn-icon.danger:hover {
    color: #ff7878;
    background: rgba(255, 80, 80, 0.1);
  }

  .btn-icon.danger.confirming {
    color: #ff7878;
    background: rgba(255, 80, 80, 0.12);
  }

  .ext-fields {
    border-top: 1px solid rgba(255, 255, 255, 0.06);
    padding: 12px 14px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    background: rgba(0, 0, 0, 0.14);
    border-radius: 0 0 10px 10px;
  }

  .ext-field {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
  }

  .ext-field-label {
    font-size: 13px;
    color: rgba(255, 255, 255, 0.75);
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }

  .field-lock {
    font-size: 12px;
    color: rgba(255, 255, 255, 0.3);
  }

  .ext-field input[type='text'],
  .ext-field input[type='password'],
  .ext-field input[type='number'],
  .ext-field select {
    background: rgba(255, 255, 255, 0.06);
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: 7px;
    color: #fff;
    padding: 7px 10px;
    font-size: 13px;
    width: 230px;
    outline: none;
    transition: border-color 0.15s;
  }

  .ext-field input:focus,
  .ext-field select:focus {
    border-color: var(--accent-color);
  }

  .ext-field select option {
    background: #1b1722;
  }

  .ext-fields-footer {
    display: flex;
    justify-content: flex-end;
    align-items: center;
    gap: 10px;
    margin-top: 2px;
  }

  .saved-hint {
    color: #9be3b3;
    font-size: 12.5px;
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }

  /* Toggle switch (matches settings panel styling) */
  .toggle-switch {
    position: relative;
    display: inline-block;
    width: 38px;
    height: 22px;
    flex-shrink: 0;
  }

  .toggle-switch.small {
    width: 34px;
    height: 20px;
  }

  .toggle-switch input {
    opacity: 0;
    width: 0;
    height: 0;
  }

  .toggle-slider {
    position: absolute;
    cursor: pointer;
    inset: 0;
    background: rgba(255, 255, 255, 0.15);
    border-radius: 99px;
    transition: background 0.2s;
  }

  .toggle-slider::before {
    content: '';
    position: absolute;
    height: 16px;
    width: 16px;
    left: 3px;
    top: 3px;
    background: #fff;
    border-radius: 50%;
    transition: transform 0.2s;
  }

  .toggle-switch.small .toggle-slider::before {
    height: 14px;
    width: 14px;
  }

  .toggle-switch input:checked + .toggle-slider {
    background: var(--accent-color, #9a6ad0);
  }

  .toggle-switch input:checked + .toggle-slider::before {
    transform: translateX(16px);
  }

  .toggle-switch.small input:checked + .toggle-slider::before {
    transform: translateX(14px);
  }

  .spinner {
    width: 13px;
    height: 13px;
    border: 2px solid rgba(255, 255, 255, 0.25);
    border-top-color: #fff;
    border-radius: 50%;
    animation: spin 0.7s linear infinite;
  }

  .spinner.small {
    width: 12px;
    height: 12px;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
