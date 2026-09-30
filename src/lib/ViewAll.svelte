<script>
import { onMount, onDestroy, createEventDispatcher } from 'svelte';
import { invoke } from '@tauri-apps/api/core';
import { scrollHoverGuard } from './utils/scrollHoverGuard.js';
import { getImageUrl, getCorsImageUrl } from './tmdb.js';
import { fetchBrowseList } from './browseLists.js';
import { myListStore } from './stores/listStore.js';
import { watchProgressStore } from './stores/watchProgressStore.js';
import { getRatingColor, extractArtAccent } from './utils/colorUtils.js';

const dispatch = createEventDispatcher();

export let title = '';
export let type = 'all';
export let category = null;
export let genre = null;
export let filterId = null;
export let customItems = null;

let items = [];
let loading = true;
let error = null;
let page = 1;
let totalPages = 1;
let cardColors = {};
let playingItemKey = null;

let showStickyHeader = false;

let overlayEl;
let mainHeaderEl;
let infiniteScrollSentinel;
let loadMoreObserver;
let scrollHost;

$: myListItems = new Set($myListStore.map(item => `${item.id}-${item.media_type}`));
$: loadCardColors(items);

// same per-title accent as the home rows; titles without colourful art keep the app accent
function loadCardColors(list) {
  for (const item of list) {
    const key = itemKey(item);
    if (key in cardColors || !item.poster_path) continue;
    cardColors[key] = null;
    extractArtAccent(getCorsImageUrl(item.poster_path, 'w92'), null).then((color) => {
      if (!color) return;
      cardColors[key] = color;
      cardColors = cardColors;
    });
  }
}
$: isTagList = category === 'discover_by_genre' || category === 'discover_by_keyword' || category === 'person';
$: if (scrollHost && infiniteScrollSentinel && !customItems && page < totalPages) {
  setupInfiniteScroll();
} else if (loadMoreObserver && (!infiniteScrollSentinel || customItems || page >= totalPages)) {
  loadMoreObserver.disconnect();
  loadMoreObserver = null;
}

onMount(async () => {
  await loadItems();

  const titlebarHeight = parseInt(
    getComputedStyle(document.documentElement).getPropertyValue('--titlebar-height') || '50',
    10,
  ) || 50;
  let scrollFrame = null;
  const updateStickyHeader = () => {
    scrollFrame = null;
    if (!mainHeaderEl) return;
    const headerRect = mainHeaderEl.getBoundingClientRect();
    showStickyHeader = headerRect.bottom <= titlebarHeight + 8;
  };
  const handleOverlayScroll = () => {
    if (scrollFrame === null) scrollFrame = requestAnimationFrame(updateStickyHeader);
  };

  scrollHost = overlayEl;
  if (scrollHost) {
    scrollHost.addEventListener('scroll', handleOverlayScroll, { passive: true });
  }
  handleOverlayScroll();
  setupInfiniteScroll();

  return () => {
    if (scrollHost) {
      scrollHost.removeEventListener('scroll', handleOverlayScroll);
    }
    if (scrollFrame !== null) cancelAnimationFrame(scrollFrame);
    if (loadMoreObserver) {
      loadMoreObserver.disconnect();
      loadMoreObserver = null;
    }
  };
});

onDestroy(() => {
  if (loadMoreObserver) {
    loadMoreObserver.disconnect();
    loadMoreObserver = null;
  }
});


async function loadItems() {
  loading = true;
  error = null;

  try {
    if (customItems) {
      items = customItems;
      loading = false;
      return;
    }

    const response = await fetchBrowseList({ category, type, filterId: filterId ?? genre, page });
    if (page === 1) {
      items = response.results;
    } else {
      const existingKeys = new Set(items.map(itemKey));
      items = [...items, ...response.results.filter(item => !existingKeys.has(itemKey(item)))];
    }
    totalPages = response.total_pages || 1;
  } catch (err) {
    error = err.message;
  }

  loading = false;
  // filtered pages can come back short and leave the sentinel on screen, which never fires again
  if (!error && page < totalPages) requestAnimationFrame(setupInfiniteScroll);
}

function itemKey(item) {
  return `${item.id}-${item.media_type}`;
}

function setupInfiniteScroll() {
  if (!scrollHost || !infiniteScrollSentinel) return;
  if (loadMoreObserver) {
    loadMoreObserver.disconnect();
  }

  loadMoreObserver = new IntersectionObserver(
    (entries) => {
      for (const entry of entries) {
        if (entry.isIntersecting) {
          loadMore();
        }
      }
    },
    {
      root: scrollHost,
      rootMargin: '600px 0px',
      threshold: 0.01,
    },
  );

  loadMoreObserver.observe(infiniteScrollSentinel);
}

function loadMore() {
  if (page < totalPages && !loading) {
    page++;
    loadItems();
  }
}

function openDetail(item) {
  // Ensure media_type is set before dispatching
  if (!item.media_type) {
    item.media_type = type === 'tv' ? 'tv' : 'movie';
  }
  window.dispatchEvent(new CustomEvent('openMediaDetail', { detail: item }));
}

async function handleQuickPlay(event, item) {
  event.stopPropagation();

  const itemMediaType = item.media_type || (type === 'tv' ? 'tv' : 'movie');
  const key = `${item.id}-${itemMediaType}`;
  if (playingItemKey === key) return; // already in-flight
  const progress = $watchProgressStore[key];
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
    detail: { ...item, autoPlay: true, resumeProgress: progress }
  }));
}

function isInMyList(item) {
  return myListItems.has(`${item.id}-${item.media_type}`);
}

function toggleMyList(event, item) {
  event.stopPropagation();
  // Ensure media_type is set before adding to list
  if (!item.media_type) {
    item.media_type = type === 'tv' ? 'tv' : 'movie';
  }
  console.log('viewall: toggle for:', item.title || item.name);
  myListStore.toggleItem(item);
}

function formatDate(dateString) {
  if (!dateString) return 'N/A';
  return new Date(dateString).getFullYear();
}

</script>

<div class="view-all-overlay" bind:this={overlayEl} use:scrollHoverGuard>

  <div class="view-all-sticky-header" class:visible={showStickyHeader}>
    <button class="btn-standard back-btn" on:click={() => dispatch('close')}>
      <i class="ri-arrow-left-line"></i>
      Back
    </button>
    {#if isTagList}
      <span class="view-all-title-pill">{title}</span>
    {:else}
      <h2>{title}</h2>
    {/if}
    <div class="header-spacer"></div>
  </div>

  <div class="view-all-container">
    <div class="view-all-header" bind:this={mainHeaderEl}>
      <button class="btn-standard back-btn" on:click={() => dispatch('close')}>
        <i class="ri-arrow-left-line"></i>
        Back
      </button>
      {#if isTagList}
        <span class="view-all-title-pill">{title}</span>
      {:else}
        <h1>{title}</h1>
      {/if}
      <div class="header-spacer"></div>
    </div>

    {#if loading && items.length === 0}
      <div class="view-all-grid" aria-hidden="true">
        {#each Array(18) as _}
          <div class="tile poster fluid rec-skeleton-pulse"></div>
        {/each}
      </div>
    {:else if error}
      <div class="error">Error: {error}</div>
    {:else}
      <div class="view-all-grid">
        {#each items as item (itemKey(item))}
          {@const itemKey = `${item.id}-${item.media_type}`}
          {@const inList = myListItems.has(itemKey)}
          <!-- svelte-ignore a11y-click-events-have-key-events -->
          <!-- svelte-ignore a11y-no-static-element-interactions -->
          <div class="tile poster fluid" style:--card-accent={cardColors[itemKey] || null} on:click={() => openDetail(item)}>
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
              <h3 class="tile-title">{item.title || item.name}</h3>
              <div class="tile-meta">
                {#if item.release_date || item.first_air_date}
                  <span>{formatDate(item.release_date || item.first_air_date)}</span>
                {/if}
                {#if item.vote_average > 0}
                  <span class="tile-rating"><i class="ri-star-fill" style="color: {getRatingColor(item.vote_average)}"></i>{item.vote_average.toFixed(1)}</span>
                {/if}
              </div>
            </div>
          </div>
        {/each}
      </div>

      {#if !customItems && loading && items.length > 0}
        <div class="load-more-container">
          <div class="loading-more-inline">Loading more...</div>
        </div>
      {/if}

      {#if !customItems && page < totalPages}
        <div class="infinite-scroll-sentinel" bind:this={infiniteScrollSentinel}></div>
      {/if}
    {/if}
  </div>
</div>
