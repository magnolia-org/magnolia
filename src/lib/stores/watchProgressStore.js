import { writable } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

// mirror of the backend's progress map, each entry carrying a `watched` flag.
// the backend owns every rule, this only keeps a synchronous copy for the ui
let _currentProgress = {};

async function loadFromDisk() {
  try {
    return await invoke('get_watch_progress') || {};
  } catch (error) {
    console.error('Error loading watch progress from disk:', error);
    return {};
  }
}

async function migrateFromLocalStorage() {
  if (typeof window === 'undefined') return;
  try {
    const stored = localStorage.getItem('watchProgress');
    if (stored) {
      const progress = JSON.parse(stored);
      if (progress && typeof progress === 'object' && Object.keys(progress).length > 0) {
        await invoke('set_watch_progress', { progress });
        console.log('✅ Migrated watch progress from localStorage to disk');
      }
      localStorage.removeItem('watchProgress');
    }
  } catch (error) {
    console.error('Error migrating watch progress from localStorage:', error);
  }
}

function createWatchProgressStore() {
  const { subscribe, set } = writable({});

  // Keep module-level mirror in sync for synchronous reads
  subscribe(val => { _currentProgress = val; });

  async function reload() {
    set(await loadFromDisk());
  }

  async function init() {
    await migrateFromLocalStorage();
    await reload();
    listen('watch-state-changed', reload).catch(error => {
      console.error('Failed to listen for watch state changes:', error);
    });
  }

  init();

  return {
    subscribe,

    // update: { mediaId, mediaType, season, episode, position, duration, completed, media }
    recordProgress: async (update) => {
      try {
        return await invoke('record_watch_progress', { update });
      } catch (error) {
        console.error('Failed to record watch progress:', error);
        return null;
      }
    },

    getEpisodeProgress: (mediaId, mediaType, season, episode) => {
      const key = `${mediaId}-${mediaType}-S${season}-E${episode}`;
      return _currentProgress[key] || null;
    },

    getProgress: (mediaId, mediaType) => {
      const key = `${mediaId}-${mediaType}`;
      return _currentProgress[key] || null;
    },

    removeProgress: async (mediaId, mediaType) => {
      const key = `${mediaId}-${mediaType}`;
      try {
        await invoke('remove_watch_progress_entry', { key });
        console.log('🗑️ Removed watch progress:', key);
      } catch (error) {
        console.error('Failed to remove watch progress:', error);
      }
    },

    clear: async () => {
      try {
        await invoke('clear_watch_progress');
        console.log('🗑️ All watch progress cleared');
      } catch (error) {
        console.error('Failed to clear watch progress:', error);
      }
    },

    reload,
  };
}

export const watchProgressStore = createWatchProgressStore();
