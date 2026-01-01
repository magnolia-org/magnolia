<script>
  import { onMount, createEventDispatcher } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { fade, scale } from 'svelte/transition';
  import { watchHistoryStore } from './stores/watchHistoryStore.js';
  import { myListStore } from './stores/listStore.js';
  
  const dispatch = createEventDispatcher();
  
  let cacheGroups = [];
  let fontStats = { count: 0, size: 0 };
  let loading = true;
  let totalSize = 0;
  let hashMappings = {};

  $: historyMap = new Map($watchHistoryStore.map(i => [i.id.toString(), i]));
  $: listMap = new Map($myListStore.map(i => [i.id.toString(), i]));
  
  let metadataCache = new Map();
  let bearerToken = null;

  async function getBearerToken() {
    if (bearerToken) {
      return bearerToken;
    }

    try {
      const response = await fetch('https://magnolia-tmdb.netlify.app/tmdb-proxy');
      const data = await response.json();
      if (data.token) {
        bearerToken = data.token;
        return bearerToken;
      }
    } catch (error) {
      console.error('failed to fetch bearer token:', error);
    }
    return null;
  }

  async function fetchMetadataFromTMDB(id, forcedMediaType = null) {
    const cacheKey = forcedMediaType ? `${id}-${forcedMediaType}` : id;
    
    if (metadataCache.has(cacheKey)) {
      return metadataCache.get(cacheKey);
    }

    // Skip if this looks like a hash (torrent hash IDs are 40 char hex strings)
    if (/^[a-f0-9]{40}$/i.test(id)) {
      // Check if we have a mapping for this hash
      const mapping = hashMappings[id.toLowerCase()];
      if (mapping) {
        const tmdbId = mapping.tmdb_id;
        const mediaType = mapping.media_type;
        console.log(`[cache metadata] using mapping ${id.substring(0, 8)}... -> ${mediaType}/${tmdbId}`);
        return fetchMetadataFromTMDB(tmdbId.toString(), mediaType);
      }
      return null;
    }

    try {
      const token = await getBearerToken();
      if (!token) {
        console.error('no bearer token available');
        return null;
      }

      const headers = {
        'Authorization': `Bearer ${token}`,
        'Content-Type': 'application/json'
      };
      
      // If we have a forced media type from mapping, use it directly
      if (forcedMediaType) {
        const endpoint = forcedMediaType === 'tv' 
          ? `https://api.themoviedb.org/3/tv/${id}`
          : `https://api.themoviedb.org/3/movie/${id}`;
        
        const response = await fetch(endpoint, { headers });

        if (response.ok) {
          const data = await response.json();
          const metadata = {
            title: forcedMediaType === 'tv' ? data.name : data.title,
            poster: data.poster_path ? `https://image.tmdb.org/t/p/w92${data.poster_path}` : null
          };
          metadataCache.set(cacheKey, metadata);
          return metadata;
        }
        return null;
      }
      
      // Try movie first
      const movieResponse = await fetch(
        `https://api.themoviedb.org/3/movie/${id}`,
        { headers }
      );

      if (movieResponse.ok) {
        const data = await movieResponse.json();
        const metadata = {
          title: data.title,
          poster: data.poster_path ? `https://image.tmdb.org/t/p/w92${data.poster_path}` : null
        };
        metadataCache.set(cacheKey, metadata);
        return metadata;
      }

      // Try TV show
      const tvResponse = await fetch(
        `https://api.themoviedb.org/3/tv/${id}`,
        { headers }
      );

      if (tvResponse.ok) {
        const data = await tvResponse.json();
        const metadata = {
          title: data.name,
          poster: data.poster_path ? `https://image.tmdb.org/t/p/w92${data.poster_path}` : null
        };
        metadataCache.set(cacheKey, metadata);
        return metadata;
      }
    } catch (e) {
      console.error('failed to fetch metadata from tmdb', e);
    }

    return null;
  }
  
  async function getMetadata(id) {
    const item = historyMap.get(id.toString()) || listMap.get(id.toString());
    if (item) {
      return {
        title: item.title || item.name,
        poster: item.poster_path ? `https://image.tmdb.org/t/p/w92${item.poster_path}` : null
      };
    }

    // If not in local stores, try fetching from TMDB
    const tmdbData = await fetchMetadataFromTMDB(id);
    if (tmdbData) {
      return tmdbData;
    }

    // If it looks like a torrent hash, show a friendly label
    if (/^[a-f0-9]{40}$/i.test(id)) {
      return { title: `Cached Media (${id.substring(0, 8)}...)`, poster: null };
    }

    return { title: `Cached Media`, poster: null };
  }

  // Aggregate all torrents into a single generic item
  $: torrentGroups = cacheGroups.filter(g => g.torrent_files > 0);
  $: torrentTotalSize = torrentGroups.reduce((acc, g) => acc + g.torrent_size, 0);
  $: torrentTotalFiles = torrentGroups.reduce((acc, g) => acc + g.torrent_files, 0);
  $: mediaGroups = cacheGroups.filter(g => g.torrent_files === 0);

  // Helper to format bytes
  function formatBytes(bytes, decimals = 2) {
    if (!+bytes) return '0 Bytes';
    const k = 1024;
    const dm = decimals < 0 ? 0 : decimals;
    const sizes = ['Bytes', 'KB', 'MB', 'GB', 'TB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return `${parseFloat((bytes / Math.pow(k, i)).toFixed(dm))} ${sizes[i]}`;
  }

  async function loadCache() {
    loading = true;
    try {
      const [groups, fonts, mappings] = await Promise.all([
        invoke('get_cache_stats'),
        invoke('get_font_stats'),
        invoke('get_all_cache_metadata')
      ]);
      
      cacheGroups = groups.sort((a, b) => b.total_size - a.total_size);
      fontStats = { count: fonts[0], size: fonts[1] };
      hashMappings = mappings;
      
      totalSize = groups.reduce((acc, g) => acc + g.total_size, 0) + fontStats.size;
    } catch (e) {
      console.error('Failed to load cache stats', e);
    } finally {
      loading = false;
    }
  }

  async function clearItem(id) {
    try {
      await invoke('clear_cache_item', { id });
      await loadCache();
    } catch (e) {
      console.error('Failed to clear item', e);
    }
  }

  async function clearAll() {
    try {
      await invoke('clear_audio_cache');
      await invoke('clear_subtitle_cache');
      await invoke('clear_fonts');
      await loadCache();
    } catch (e) {
      console.error('failed to clear all', e);
    }
  }

  async function clearTorrents() {
    try {
      for (const group of torrentGroups) {
        await invoke('clear_cache_item', { id: group.id });
      }
      await loadCache();
    } catch (e) {
      console.error('failed to clear torrents', e);
    }
  }

  onMount(loadCache);
</script>

<div class="modal-overlay" on:click={() => dispatch('close')} transition:fade>
  <div class="modal-content" on:click|stopPropagation transition:scale>
    <div class="modal-header">
      <h3>Storage Manager</h3>
    </div>
    
    <div class="modal-body">
      <div class="stats-summary">
        <div class="stat-item">
          <span class="label">Total Cache Size</span>
          <span class="value">{formatBytes(totalSize)}</span>
        </div>
        <button class="btn-standard danger" on:click={clearAll}>
          <i class="ri-delete-bin-line"></i> Clear All
        </button>
      </div>

      <div class="cache-list">
        {#if loading}
          <div class="loading">Loading cache stats...</div>
        {:else}
          <!-- Fonts Section -->
          {#if fontStats.size > 0}
            <div class="cache-group global-section">
              <div class="group-header">
                <div class="group-info">
                  <div class="group-icon">
                    <i class="ri-font-size"></i>
                  </div>
                  <div class="group-details">
                    <span class="group-title">Global Fonts</span>
                    <span class="group-meta">{fontStats.count} files • {formatBytes(fontStats.size)}</span>
                  </div>
                </div>
              </div>
            </div>
          {/if}

          <!-- Torrents Section -->
          {#if torrentTotalSize > 0}
            <div class="cache-group global-section">
              <div class="group-header">
                <div class="group-info">
                  <div class="group-icon">
                    <i class="ri-folder-download-line"></i>
                  </div>
                  <div class="group-details">
                    <span class="group-title">Torrents</span>
                    <span class="group-meta">{torrentTotalFiles} items • {formatBytes(torrentTotalSize)}</span>
                  </div>
                </div>
                <button class="btn-icon danger" on:click={clearTorrents} title="Clear">
                  <i class="ri-delete-bin-line"></i>
                </button>
              </div>
            </div>
          {/if}

          {#if mediaGroups.length === 0 && fontStats.size === 0 && torrentTotalSize === 0}
            <div class="empty-state">Cache is empty</div>
          {:else}
            {#each mediaGroups as group}
              {#await getMetadata(group.id) then meta}
                <div class="cache-group">
                  <div class="group-header">
                    <div class="group-info">
                      {#if meta.poster}
                        <img src={meta.poster} alt={meta.title} class="group-poster" />
                      {:else}
                        <div class="group-poster-placeholder">
                          <i class="ri-movie-2-line"></i>
                        </div>
                      {/if}
                      <div class="group-details">
                        <span class="group-title">{meta.title}</span>
                        <span class="group-meta">{formatBytes(group.total_size)}</span>
                      </div>
                    </div>
                    <button class="btn-icon danger" on:click={() => clearItem(group.id)} title="Clear">
                      <i class="ri-delete-bin-line"></i>
                    </button>
                  </div>
                  
                  <div class="group-files">
                  {#if group.audio_files > 0}
                    <div class="file-type">
                      <i class="ri-volume-up-line"></i>
                      <span>Audio ({group.audio_files})</span>
                      <span class="file-size">{formatBytes(group.audio_size)}</span>
                    </div>
                  {/if}
                  {#if group.subtitle_files > 0}
                    <div class="file-type">
                      <i class="ri-closed-captioning-line"></i>
                      <span>Subtitles ({group.subtitle_files})</span>
                      <span class="file-size">{formatBytes(group.subtitle_size)}</span>
                    </div>
                  {/if}
                </div>
              </div>
              {/await}
            {/each}
          {/if}
        {/if}
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
    max-width: 800px;
    max-height: 85vh;
    border-radius: var(--border-radius-lg);
    border: 1px solid rgba(255, 255, 255, 0.08);
    display: flex;
    flex-direction: column;
    box-shadow: var(--shadow-depth), 0 20px 60px rgba(0, 0, 0, 0.6);
    overflow: hidden;
  }

  .modal-header {
    padding: var(--spacing-xl) var(--spacing-2xl);
    border-bottom: 1px solid rgba(255, 255, 255, 0.06);
    display: flex;
    justify-content: space-between;
    align-items: center;
    background: var(--bg-secondary);
  }

  .modal-header h3 {
    margin: 0;
    font-size: 18px;
    font-weight: 600;
    color: var(--text-primary);
    letter-spacing: -0.01em;
  }

  .modal-body {
    padding: 20px;
    overflow-y: auto;
  }

  .stats-summary {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 20px;
    padding: 16px;
    background: rgba(255, 255, 255, 0.05);
    border-radius: 12px;
  }

  .stat-item {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .stat-item .label {
    font-size: 12px;
    color: rgba(255, 255, 255, 0.5);
  }

  .stat-item .value {
    font-size: 18px;
    font-weight: 600;
    color: var(--accent-color);
  }

  .cache-list {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .loading, .empty-state {
    text-align: center;
    padding: 20px;
    color: rgba(255, 255, 255, 0.5);
    font-size: 14px;
  }

  .cache-group {
    background: rgba(255, 255, 255, 0.03);
    border-radius: 12px;
    overflow: hidden;
    transition: background 0.2s;
  }

  .cache-group:hover {
    background: rgba(255, 255, 255, 0.06);
  }

  .cache-group.global-section:hover {
    background: rgba(255, 255, 255, 0.03);
  }

  .group-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 12px;
  }

  .group-info {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .group-poster {
    width: 40px;
    height: 60px;
    object-fit: cover;
    border-radius: 4px;
  }

  .group-poster-placeholder, .group-icon {
    width: 40px;
    height: 60px;
    background: rgba(255, 255, 255, 0.1);
    border-radius: 4px;
    display: flex;
    align-items: center;
    justify-content: center;
    color: rgba(255, 255, 255, 0.3);
    font-size: 20px;
  }
  
  .group-icon {
    height: 40px;
    border-radius: 50%;
  }

  .group-details {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .group-title {
    font-size: 15px;
    font-weight: 500;
    color: var(--text-primary);
  }

  .group-meta {
    font-size: 12px;
    color: rgba(255, 255, 255, 0.5);
  }

  .group-files {
    padding: 0 12px 12px 64px; /* Indent to align with text */
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .file-type {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    color: rgba(255, 255, 255, 0.7);
  }

  .file-type i {
    color: rgba(255, 255, 255, 0.4);
  }

  .file-size {
    margin-left: auto;
    color: rgba(255, 255, 255, 0.4);
    font-size: 12px;
  }

  .btn-icon {
    background: transparent;
    border: none;
    color: rgba(255, 255, 255, 0.4);
    cursor: pointer;
    padding: 8px;
    border-radius: 8px;
    transition: all 0.2s;
  }

  .btn-icon:hover {
    background: rgba(255, 255, 255, 0.1);
    color: #ff6b6b;
  }

  .btn-standard.danger {
    background: rgba(255, 107, 107, 0.1);
    border-color: rgba(255, 107, 107, 0.3);
    color: #ff6b6b;
  }

  .btn-standard.danger:hover {
    background: rgba(255, 107, 107, 0.2);
  }
</style>

