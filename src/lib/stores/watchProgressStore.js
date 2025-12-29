import { writable } from 'svelte/store';

function loadFromLocalStorage() {
  if (typeof window !== 'undefined') {
    try {
      const savedProgress = localStorage.getItem('watchProgress');
      return savedProgress ? JSON.parse(savedProgress) : {};
    } catch (error) {
      console.error('Error loading watch progress from localStorage:', error);
      return {};
    }
  }
  return {};
}

function createWatchProgressStore() {
  const { subscribe, set, update } = writable(loadFromLocalStorage());

  return {
    subscribe,

    updateProgress: (mediaId, mediaType, data) => {
      update(progress => {
        const key = `${mediaId}-${mediaType}`;
        progress[key] = {
          ...data,
          updatedAt: Date.now()
        };

        // Also store episode-specific progress if available
        if (data.currentSeason && data.currentEpisode) {
          const episodeKey = `${mediaId}-${mediaType}-S${data.currentSeason}-E${data.currentEpisode}`;
          progress[episodeKey] = {
            ...data,
            updatedAt: Date.now()
          };
        }
        
        if (typeof window !== 'undefined') {
          localStorage.setItem('watchProgress', JSON.stringify(progress));
        }
        
        console.log('📊 Updated watch progress:', key, data);
        return progress;
      });
    },

    getEpisodeProgress: (mediaId, mediaType, season, episode) => {
      const progress = loadFromLocalStorage();
      const key = `${mediaId}-${mediaType}-S${season}-E${episode}`;
      return progress[key] || null;
    },

    getProgress: (mediaId, mediaType) => {
      const progress = loadFromLocalStorage();
      const key = `${mediaId}-${mediaType}`;
      return progress[key] || null;
    },

    removeProgress: (mediaId, mediaType) => {
      update(progress => {
        const key = `${mediaId}-${mediaType}`;
        delete progress[key];
        
        if (typeof window !== 'undefined') {
          localStorage.setItem('watchProgress', JSON.stringify(progress));
        }
        
        console.log('🗑️ Removed watch progress:', key);
        return progress;
      });
    },

    clear: () => {
      set({});
      if (typeof window !== 'undefined') {
        localStorage.removeItem('watchProgress');
      }
      console.log('🗑️ All watch progress cleared');
    },
    
    reload: () => {
      set(loadFromLocalStorage());
    }
  };
}

export const watchProgressStore = createWatchProgressStore();

if (typeof window !== 'undefined') {
  window.addEventListener('storage', (e) => {
    if (e.key === 'watchProgress') {
      watchProgressStore.reload();
    }
  });
}
