<script>
import { onMount, onDestroy, createEventDispatcher } from 'svelte';
import { invoke } from '@tauri-apps/api/core';
import { getImageUrl, getCorsImageUrl } from './tmdb.js';
import { fetchBrowseList } from './browseLists.js';
import { myListStore } from './stores/listStore.js';
import { getRatingColor, extractArtAccent } from './utils/colorUtils.js';
import Scroller from './Scroller.svelte';
import { getTitleLogo } from './titleLogos.js';

const dispatch = createEventDispatcher();

export let title = 'Section Title';
export let category = null;
export let accentColor = '#6366f1';
export let customItems = null;
export let showClearButton = false;
export let hideViewAll = false;
export let isRecentlyWatched = false;
export let watchProgress = {};
// 'poster' for the tall 2:3 cards, 'wide' for 16:9 backdrop tiles
export let variant = 'poster';
export let fallbackMediaType = 'movie';

let items = [];
let loading = true;
let error = null;
let cardColors = {};
let playingItemKey = null;
let colorFlushFrame = null;
// title logos for the wide tiles, keyed like cardColors; null once we know there is none
let logos = {};

$: wide = variant === 'wide';

function getItemKey(item) {
  return `${item.id}-${item.media_type || fallbackMediaType}`;
}

function colorSource(item) {
  if (wide && item.backdrop_path) return getCorsImageUrl(item.backdrop_path, 'w300');
  return item.poster_path ? getCorsImageUrl(item.poster_path, 'w92') : null;
}

$: myListItems = new Set($myListStore.map(item => `${item.id}-${item.media_type}`));

$: if (wide) loadLogos(items);

function loadLogos(list) {
  for (const item of list) {
    const key = getItemKey(item);
    if (key in logos) continue;
    logos[key] = undefined;
    getTitleLogo({ ...item, media_type: item.media_type || fallbackMediaType }).then((logo) => {
      logos[key] = logo;
      logos = logos;
    });
  }
}

$: if (customItems) {
  items = customItems;
  loading = false;
  // prune colors for items no longer in the list
  const activeKeys = new Set(customItems.map(getItemKey));
  let pruned = false;
  for (const key of Object.keys(cardColors)) {
    if (!activeKeys.has(key)) { delete cardColors[key]; pruned = true; }
  }
  // only extract colors for items we haven't processed yet
  customItems.forEach(item => {
    const itemKey = getItemKey(item);
    if (cardColors[itemKey]) return;
    const source = colorSource(item);
    if (source) {
      extractDominantColor(itemKey, source);
    } else {
      cardColors[itemKey] = accentColor;
    }
  });
  if (pruned) cardColors = cardColors;
}

onMount(async () => {
  if (customItems || !category) return;

  try {
    const response = await fetchBrowseList({ category });
    items = response.results;
    items.forEach(item => {
      const source = colorSource(item);
      if (source) extractDominantColor(getItemKey(item), source);
    });
  } catch (err) {
    console.error('Error fetching TMDB data:', err);
    error = err.message;
  }
  loading = false;
});

onDestroy(() => {
  if (colorFlushFrame !== null) cancelAnimationFrame(colorFlushFrame);
});

function formatRating(rating) {
  return rating ? rating.toFixed(1) : 'N/A';
}

function formatYear(dateStr) {
  if (!dateStr) return '';
  return new Date(dateStr).getFullYear();
}

function flushCardColors() {
  if (colorFlushFrame !== null) return;
  colorFlushFrame = requestAnimationFrame(() => {
    colorFlushFrame = null;
    cardColors = cardColors;
  });
}

async function extractDominantColor(itemKey, imageUrl) {
  const color = await extractArtAccent(imageUrl, accentColor);
  // a failed load leaves it unset so the section accent shows and a later list update can retry
  if (!color) return;
  cardColors[itemKey] = color;
  flushCardColors();
}

function openDetail(item) {
  // when opening detail (not quick play), strip episode tracking info
  // so it doesn't auto-navigate to a specific episode
  const cleanItem = { ...item, media_type: item.media_type || fallbackMediaType };
  delete cleanItem.current_season;
  delete cleanItem.current_episode;
  delete cleanItem.current_timestamp;
  delete cleanItem.currentSeason;
  delete cleanItem.currentEpisode;
  delete cleanItem.currentTimestamp;
  delete cleanItem.watched_at;
  delete cleanItem.watchedAt;

  window.dispatchEvent(new CustomEvent('openMediaDetail', { detail: cleanItem }));
}

function toggleMyList(event, item) {
  event.stopPropagation();
  myListStore.toggleItem({ ...item, media_type: item.media_type || fallbackMediaType });
}

async function handleQuickPlay(event, item) {
  event.stopPropagation();

  const itemMediaType = item.media_type || fallbackMediaType;
  const key = `${item.id}-${itemMediaType}`;
  if (playingItemKey === key) return; // already in-flight
  const progress = watchProgress[key];
  playingItemKey = key;

  // the backend picks the episode to continue and readies its saved torrent;
  // with nothing saved (or a finished episode) the detail view has to decide
  try {
    const plan = await invoke('plan_quick_play', { mediaId: Number(item.id), mediaType: itemMediaType });
    if (plan) {
      const mediaTitle = item.title || item.name || '';
      const isMovie = itemMediaType === 'movie';
      window.dispatchEvent(new CustomEvent('openVideoPlayer', {
        detail: {
          src: null,
          title: isMovie ? mediaTitle : `${mediaTitle} - S${plan.season}E${plan.episode}`,
          metadata: item,
          handleId: plan.handle_id,
          fileIndex: plan.file_index,
          magnetLink: plan.magnet_link,
          initialTimestamp: plan.initial_timestamp,
          mediaId: item.id,
          mediaType: itemMediaType,
          seasonNum: plan.season,
          episodeNum: plan.episode,
        }
      }));
      playingItemKey = null;
      return;
    }
  } catch (err) {
    console.warn('Quick play: saved selection check failed, falling back to detail view', err);
  }

  playingItemKey = null;
  window.dispatchEvent(new CustomEvent('openMediaDetail', {
    detail: { ...item, media_type: itemMediaType, autoPlay: true, resumeProgress: progress }
  }));
}

function handleRemoveFromHistory(event, item) {
  event.stopPropagation();
  dispatch('removeItem', { id: item.id, media_type: item.media_type });
}

function getProgressEntry(item) {
  const key = `${item.id}-${item.media_type}`;
  return watchProgress?.[key] || (item.current_season || item.currentSeason || item.current_timestamp ? item : null);
}

function formatDuration(seconds) {
  const totalMinutes = Math.max(1, Math.round(seconds / 60));
  const hours = Math.floor(totalMinutes / 60);
  const minutes = totalMinutes % 60;
  return hours > 0 ? `${hours}h ${minutes}m` : `${minutes}m`;
}

function getProgressInfo(item) {
  const progress = getProgressEntry(item);
  if (!progress) return null;

  // handle both snake_case (from rust) and camelCase (from js)
  const season = progress.current_season || progress.currentSeason;
  const episode = progress.current_episode || progress.currentEpisode;
  const timestamp = progress.current_timestamp || progress.currentTimestamp;
  const duration = progress.duration;
  const remaining = timestamp > 60 && duration > timestamp ? `${formatDuration(duration - timestamp)} left` : null;

  if ((item.media_type === 'tv' || item.first_air_date) && season && episode) {
    const label = `S${season} · E${episode}`;
    return remaining ? `${label} · ${remaining}` : label;
  }

  if (remaining) return remaining;
  if (timestamp > 60) return formatDuration(timestamp);
  return null;
}

function getProgressPercentage(item) {
  const progress = getProgressEntry(item);
  const timestamp = progress?.currentTimestamp || progress?.current_timestamp;
  if (!timestamp || !progress.duration) return 0;
  return Math.min(100, Math.max(0, (timestamp / progress.duration) * 100));
}

function handleViewAll() {
  window.dispatchEvent(new CustomEvent('viewAll', { detail: { title, category, customItems } }));
}
</script>

<div class="carousel-section" class:wide-section={wide}>
  <div class="section-header">
    <h2 class="section-title">{title}</h2>
    <div class="header-actions">
      {#if showClearButton}
        <button class="btn-standard clear-btn" on:click={() => dispatch('clear')} title="Clear Recently Watched">
          <i class="ri-delete-bin-line"></i>
          Clear
        </button>
      {/if}
      {#if !hideViewAll && !loading && !error && items.length > 0}
        <button class="btn-standard view-all" on:click={handleViewAll}>
          View All
          <i class="ri-arrow-right-s-line"></i>
        </button>
      {/if}
    </div>
  </div>

  {#if loading}
    <div class="carousel-skeleton" aria-hidden="true">
      {#each Array(8) as _}
        <div class="skeleton-card rec-skeleton-pulse" class:wide></div>
      {/each}
    </div>
  {:else if error}
    <div class="carousel-error">
      <i class="ri-wifi-off-line"></i>
      Couldn't load this list
    </div>
  {:else if items.length === 0}
    <div class="carousel-error">
      <i class="ri-inbox-line"></i>
      Nothing here right now
    </div>
  {:else}
    <Scroller gap={wide ? 'var(--spacing-xl)' : 'var(--spacing-lg)'}>
      {#each items as item, index (`${item.id}-${item.media_type}-${index}`)}
        {@const itemKey = getItemKey(item)}
        {@const inList = myListItems.has(itemKey)}
        {#if wide}
          {@const progressInfo = isRecentlyWatched && !item.finished ? getProgressInfo(item) : null}
          {@const progressPercent = isRecentlyWatched && !item.finished ? getProgressPercentage(item) : 0}
          <!-- svelte-ignore a11y-click-events-have-key-events -->
          <!-- svelte-ignore a11y-no-static-element-interactions -->
          <div class="tile wide" class:finished={isRecentlyWatched && item.finished} style="--card-accent: {cardColors[itemKey] || accentColor}" on:click={() => openDetail(item)}>
            <div class="tile-art">
              {#if item.backdrop_path}
                <img class="tile-image" src={getImageUrl(item.backdrop_path, 'w780')} alt="" loading="lazy" decoding="async" />
              {:else if item.poster_path}
                <!-- no backdrop: a blurred poster fills the frame and the poster itself sits on the right -->
                <img class="tile-image poster-blur" src={getImageUrl(item.poster_path, 'w185')} alt="" loading="lazy" decoding="async" />
                <img class="wide-poster" src={getImageUrl(item.poster_path, 'w342')} alt="" loading="lazy" decoding="async" />
              {:else}
                <div class="tile-image tile-placeholder"><i class="ri-film-line"></i></div>
              {/if}
            </div>
            <div class="tile-shade"></div>

            {#if isRecentlyWatched && item.finished}
              <div class="tile-badge"><i class="ri-check-line"></i> Watched</div>
            {/if}

            <div class="tile-corner">
              {#if isRecentlyWatched}
                <button class="tile-icon" title="Remove from Recently Watched" on:click={(e) => handleRemoveFromHistory(e, item)}>
                  <i class="ri-close-line"></i>
                </button>
              {:else}
                <button class="tile-icon" class:active={inList} title={inList ? 'Remove from List' : 'Add to List'} on:click={(e) => toggleMyList(e, item)}>
                  <i class={inList ? 'ri-check-line' : 'ri-add-line'}></i>
                </button>
              {/if}
            </div>

            <button class="tile-play" class:loading={playingItemKey === itemKey} title={isRecentlyWatched && !item.finished ? 'Resume' : 'Play'} disabled={playingItemKey !== null} on:click={(e) => handleQuickPlay(e, item)}>
              {#if playingItemKey === itemKey}
                <i class="ri-loader-4-line spin"></i>
              {:else}
                <i class="ri-play-fill"></i>
              {/if}
            </button>

            <div class="tile-text">
              {#if logos[itemKey]}
                <img class="tile-logo" class:invert={logos[itemKey].invert} src={logos[itemKey].url} alt={item.title || item.name} decoding="async" />
              {:else}
                <h3 class="tile-title" class:pending={logos[itemKey] === undefined}>{item.title || item.name || 'Unknown'}</h3>
              {/if}
              <div class="tile-meta">
                {#if progressInfo}
                  <span>{progressInfo}</span>
                {:else}
                  {#if formatYear(item.release_date || item.first_air_date)}
                    <span>{formatYear(item.release_date || item.first_air_date)}</span>
                  {/if}
                  {#if item.vote_average}
                    <span class="tile-rating"><i class="ri-star-fill" style="color: {getRatingColor(item.vote_average)}"></i>{formatRating(item.vote_average)}</span>
                  {/if}
                {/if}
              </div>
              {#if progressPercent > 0}
                <div class="tile-progress"><span style:width="{progressPercent}%"></span></div>
              {/if}
            </div>
          </div>
        {:else}
          <!-- svelte-ignore a11y-click-events-have-key-events -->
          <!-- svelte-ignore a11y-no-static-element-interactions -->
          <div class="tile poster" style="--card-accent: {cardColors[itemKey] || accentColor}" on:click={() => openDetail(item)}>
            <div class="tile-art">
              {#if item.poster_path}
                <img class="tile-image" src={getImageUrl(item.poster_path, 'w342')} alt={item.title || item.name} loading="lazy" decoding="async" />
              {:else}
                <div class="tile-image tile-placeholder"><i class="ri-film-line"></i></div>
              {/if}
            </div>
            <div class="tile-shade"></div>

            <div class="tile-corner">
              <button class="tile-icon" class:active={inList} title={inList ? 'Remove from List' : 'Add to List'} on:click={(e) => toggleMyList(e, item)}>
                <i class={inList ? 'ri-check-line' : 'ri-add-line'}></i>
              </button>
            </div>

            <button class="tile-play" class:loading={playingItemKey === itemKey} title="Play" disabled={playingItemKey !== null} on:click={(e) => handleQuickPlay(e, item)}>
              {#if playingItemKey === itemKey}
                <i class="ri-loader-4-line spin"></i>
              {:else}
                <i class="ri-play-fill"></i>
              {/if}
            </button>

            <div class="tile-text">
              <h3 class="tile-title">{item.title || item.name || 'Unknown'}</h3>
              <div class="tile-meta">
                {#if formatYear(item.release_date || item.first_air_date)}
                  <span>{formatYear(item.release_date || item.first_air_date)}</span>
                {/if}
                {#if item.vote_average}
                  <span class="tile-rating"><i class="ri-star-fill" style="color: {getRatingColor(item.vote_average)}"></i>{formatRating(item.vote_average)}</span>
                {/if}
              </div>
            </div>
          </div>
        {/if}
      {/each}
    </Scroller>
  {/if}
</div>
