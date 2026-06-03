import { writable } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';

// Mirror of current store value for synchronous getProgress/getEpisodeProgress reads
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
  const { subscribe, set, update } = writable({});

  // Keep module-level mirror in sync for synchronous reads
  subscribe(val => { _currentProgress = val; });

  async function init() {
    await migrateFromLocalStorage();
    const progress = await loadFromDisk();
    set(progress);
  }

  init();

  return {
    subscribe,

    updateProgress: async (mediaId, mediaType, data) => {
      const key = `${mediaId}-${mediaType}`;
      const entry = { ...data, updatedAt: Date.now() };

      // Update in-memory immediately so UI stays responsive
      update(progress => {
        const next = { ...progress, [key]: entry };
        if (data.currentSeason && data.currentEpisode) {
          const epKey = `${mediaId}-${mediaType}-S${data.currentSeason}-E${data.currentEpisode}`;
          next[epKey] = entry;
        }
        return next;
      });

      try {
        await invoke('update_watch_progress_entry', { key, value: entry });
        if (data.currentSeason && data.currentEpisode) {
          const epKey = `${mediaId}-${mediaType}-S${data.currentSeason}-E${data.currentEpisode}`;
          await invoke('update_watch_progress_entry', { key: epKey, value: entry });
        }
        console.log('📊 Updated watch progress:', key, data);
      } catch (error) {
        console.error('Failed to update watch progress:', error);
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
      update(progress => {
        const next = { ...progress };
        delete next[key];
        return next;
      });
      try {
        await invoke('remove_watch_progress_entry', { key });
        console.log('🗑️ Removed watch progress:', key);
      } catch (error) {
        console.error('Failed to remove watch progress:', error);
      }
    },

    clear: async () => {
      set({});
      try {
        await invoke('clear_watch_progress');
        console.log('🗑️ All watch progress cleared');
      } catch (error) {
        console.error('Failed to clear watch progress:', error);
      }
    },

    reload: async () => {
      const progress = await loadFromDisk();
      set(progress);
    },
  };
}

export const watchProgressStore = createWatchProgressStore();
