import { invoke } from '@tauri-apps/api/core';

// tracker extension ids to search, empty for auto. saved in the backend settings

async function migrateFromLocalStorage() {
  if (typeof window === 'undefined') return;
  const stored = localStorage.getItem('trackerPreference');
  if (stored === null) return;
  localStorage.removeItem('trackerPreference');
  try {
    const parsed = JSON.parse(stored);
    if (Array.isArray(parsed) && parsed.length > 0) {
      await invoke('set_tracker_preference', { trackers: parsed });
    }
  } catch (e) {
    // old builds stored a plain string here, which means auto
  }
}

const migration = migrateFromLocalStorage();

export async function getTrackerPreference() {
  await migration;
  try {
    return await invoke('get_tracker_preference');
  } catch (error) {
    console.error('failed to load tracker preference:', error);
    return [];
  }
}

export async function setTrackerPreference(trackers) {
  try {
    await invoke('set_tracker_preference', { trackers: Array.isArray(trackers) ? trackers : [] });
  } catch (error) {
    console.error('failed to save tracker preference:', error);
  }
}
