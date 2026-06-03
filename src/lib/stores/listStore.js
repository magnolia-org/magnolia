import { writable } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';

async function loadFromDisk() {
  try {
    return await invoke('get_my_list') || [];
  } catch (error) {
    console.error('Error loading my list from disk:', error);
    return [];
  }
}

async function migrateFromLocalStorage() {
  if (typeof window === 'undefined') return;
  try {
    const stored = localStorage.getItem('myList');
    if (stored) {
      const items = JSON.parse(stored);
      if (Array.isArray(items) && items.length > 0) {
        await invoke('set_my_list', { items });
        console.log('✅ Migrated my list from localStorage to disk');
      }
      localStorage.removeItem('myList');
    }
  } catch (error) {
    console.error('Error migrating my list from localStorage:', error);
  }
}

function createListStore() {
  const { subscribe, set } = writable([]);

  async function init() {
    await migrateFromLocalStorage();
    const list = await loadFromDisk();
    set(list);
  }

  init();

  return {
    subscribe,

    toggleItem: async (item) => {
      try {
        const newList = await invoke('toggle_my_list_item', { item });
        set(newList);
        console.log('📋 Toggled list item:', item.title || item.name);
      } catch (error) {
        console.error('Failed to toggle list item:', error);
      }
    },

    isInList: (item, list) => {
      return list.some(media =>
        media.id === item.id && media.media_type === item.media_type
      );
    },

    setList: async (newList) => {
      try {
        await invoke('set_my_list', { items: newList });
        set(newList);
      } catch (error) {
        console.error('Failed to set list:', error);
      }
    },

    reload: async () => {
      const list = await loadFromDisk();
      set(list);
    },
  };
}

export const myListStore = createListStore();

