// an entry counts as watched once playback is near the end or the player saw it finish
const WATCHED_RATIO = 0.9;
const WATCHED_REMAINING_SECONDS = 180;

export function getWatchRatio(entry) {
  if (!entry?.duration) return 0;
  return Math.min(1, Math.max(0, (entry.currentTimestamp || 0) / entry.duration));
}

export function isEntryWatched(entry) {
  if (!entry) return false;
  if (entry.completed) return true;
  if (!entry.duration) return false;
  const ratio = getWatchRatio(entry);
  const remaining = entry.duration - (entry.currentTimestamp || 0);
  return ratio >= WATCHED_RATIO || (ratio >= 0.5 && remaining <= WATCHED_REMAINING_SECONDS);
}

function progressKey(item) {
  return `${item.id}-${item.media_type}`;
}

// the progress entry that decides whether the whole title is done, or null if it can't be
function getFinishingEntry(item, progress) {
  const key = progressKey(item);
  const entry = progress?.[key];
  if (!entry) return null;
  if (item.media_type !== 'tv') return entry;

  const season = entry.currentSeason;
  const episode = entry.currentEpisode;
  if (!season || !episode) return null;

  // without the series length a finale looks like any other episode
  const lastSeason = item.last_season_number ?? item.number_of_seasons;
  const lastEpisodeCount = item.last_season_episode_count;
  if (!lastSeason || !lastEpisodeCount) return null;
  if (season !== lastSeason || episode < lastEpisodeCount) return null;

  return progress[`${key}-S${season}-E${episode}`] || entry;
}

export function isMediaFinished(item, progress) {
  return isEntryWatched(getFinishingEntry(item, progress));
}

// unfinished titles by when they were last watched, then finished titles by when they were finished
export function sortWatchHistory(history, progress) {
  const unfinished = [];
  const finished = [];
  for (const item of history || []) {
    const entry = progress?.[progressKey(item)];
    const lastWatched = Math.max(item.watched_at || 0, entry?.updatedAt || 0);
    const finishing = getFinishingEntry(item, progress);
    if (isEntryWatched(finishing)) {
      finished.push({ item, time: finishing.completedAt || finishing.updatedAt || lastWatched });
    } else {
      unfinished.push({ item, time: lastWatched });
    }
  }
  const byNewest = (a, b) => b.time - a.time;
  return [...unfinished.sort(byNewest), ...finished.sort(byNewest)].map(e => e.item);
}
