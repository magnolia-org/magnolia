import { writable } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

// the backend adds titles while recording progress and returns them already
// sorted for display, each flagged `finished` once the whole title is watched
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
      number_of_seasons: item.number_of_seasons ?? null,
      last_season_number: item.last_season_number ?? null,
      last_season_episode_count: item.last_season_episode_count ?? null,
      finished: !!item.finished,
    }));
  } catch (error) {
    console.error('Error loading watch history from disk:', error);
    return [];
  }
}

function createWatchHistoryStore() {
  const { subscribe, set } = writable([]);

  async function reload() {
    set(await loadFromDisk());
  }

  reload();
  listen('watch-state-changed', reload).catch(error => {
    console.error('Failed to listen for watch state changes:', error);
  });

  return {
    subscribe,

    removeItem: async (mediaId, mediaType) => {
      try {
        await invoke('remove_watch_history_item', { mediaId, mediaType });
        console.log('🗑️ Removed from watch history:', mediaId);
      } catch (error) {
        console.error('Failed to remove watch history item:', error);
      }
    },

    clear: async () => {
      try {
        await invoke('clear_watch_history');
        console.log('🗑️ Watch history cleared');
      } catch (error) {
        console.error('Failed to clear watch history:', error);
      }
    },

    reload,
  };
}

export const watchHistoryStore = createWatchHistoryStore();
