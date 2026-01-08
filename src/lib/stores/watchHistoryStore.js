import { writable } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';

async function loadFromDisk() {
  try {
    const history = await invoke('get_watch_history');
    // Convert snake_case from Rust to camelCase for JS
    return (history || []).map(item => ({
      id: item.id,
      media_type: item.media_type,
      title: item.title,
      poster_path: item.poster_path,
      backdrop_path: item.backdrop_path,
      release_date: item.release_date,
      vote_average: item.vote_average,
      watchedAt: item.watched_at,
      // Keep both formats for compatibility
      watched_at: item.watched_at,
      currentSeason: item.current_season,
      currentEpisode: item.current_episode,
      currentTimestamp: item.current_timestamp,
      current_season: item.current_season,
      current_episode: item.current_episode,
      current_timestamp: item.current_timestamp,
    }));
  } catch (error) {
    console.error('Error loading watch history from disk:', error);
    return [];
  }
}

function createWatchHistoryStore() {
  const { subscribe, set, update } = writable([]);

  // Load initial data from disk
  loadFromDisk().then(history => set(history));

  return {
    subscribe,

    addItem: async (item, episodeData = null) => {
      const historyItem = {
        id: item.id,
        media_type: item.media_type,
        title: item.title || item.name || 'Unknown',
        poster_path: item.poster_path || null,
        backdrop_path: item.backdrop_path || null,
        release_date: item.release_date || item.first_air_date || null,
        vote_average: item.vote_average || null,
        watched_at: Date.now(),
        current_season: episodeData?.season || null,
        current_episode: episodeData?.episode || null,
        current_timestamp: episodeData?.timestamp || null,
      };

      try {
        await invoke('add_watch_history_item', { item: historyItem });
        const updatedHistory = await loadFromDisk();
        set(updatedHistory);
        console.log('📺 Added to watch history:', historyItem.title, episodeData);
      } catch (error) {
        console.error('Failed to add watch history item:', error);
      }
    },

    removeItem: async (mediaId, mediaType) => {
      try {
        await invoke('remove_watch_history_item', { mediaId, mediaType });
        const updatedHistory = await loadFromDisk();
        set(updatedHistory);
        console.log('🗑️ Removed from watch history:', mediaId);
      } catch (error) {
        console.error('Failed to remove watch history item:', error);
      }
    },

    clear: async () => {
      try {
        await invoke('clear_watch_history');
        set([]);
        console.log('🗑️ Watch history cleared');
      } catch (error) {
        console.error('Failed to clear watch history:', error);
      }
    },
    
    reload: async () => {
      const history = await loadFromDisk();
      set(history);
    }
  };
}

export const watchHistoryStore = createWatchHistoryStore();

// Tracker preference utilities (keep using localStorage for these)
export function getTrackerPreference() {
  if (typeof window !== 'undefined') {
    try {
      const stored = localStorage.getItem('trackerPreference');
      if (stored) {
        const parsed = JSON.parse(stored);
        return Array.isArray(parsed) ? parsed : [];
      }
    } catch (e) {
      // Migration: if it's an old string value, clear it
      localStorage.removeItem('trackerPreference');
    }
  }
  return [];
}

export function setTrackerPreference(trackers) {
  if (typeof window !== 'undefined') {
    const trackersArray = Array.isArray(trackers) ? trackers : [];
    localStorage.setItem('trackerPreference', JSON.stringify(trackersArray));
    console.log('tracker preference set to:', trackersArray);
  }
}
