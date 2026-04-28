<script>
import { onMount, onDestroy, createEventDispatcher } from 'svelte';
import { getTrending, getPopularMovies, getPopularTV, getTopRatedMovies, getTopRatedTV, getNowPlaying, discoverMovies, discoverTV, getImageUrl } from './tmdb.js';
import { myListStore } from './stores/listStore.js';
import { getRatingClass } from './utils/colorUtils.js';

const dispatch = createEventDispatcher();

export let title = '';
export let type = 'movie';
export let category = 'popular';
export let genre = null;
export let filterId = null;
export let customItems = null;

let items = [];
let loading = true;
let error = null;
let page = 1;
let totalPages = 1;
let cardColors = {};

let showStickyHeader = false;

let overlayEl;
let mainHeaderEl;
let infiniteScrollSentinel;
let loadMoreObserver;
let scrollHost;

$: myListItems = new Set($myListStore.map(item => `${item.id}-${item.media_type}`));
$: isTagList = category === 'discover_by_genre' || category === 'discover_by_keyword';
$: if (scrollHost && infiniteScrollSentinel && !customItems && page < totalPages) {
  setupInfiniteScroll();
} else if (loadMoreObserver && (!infiniteScrollSentinel || customItems || page >= totalPages)) {
  loadMoreObserver.disconnect();
  loadMoreObserver = null;
}

onMount(async () => {
  await loadItems();

  const handleMouseButton = (e) => {
    if (e.button === 3) { // Back button
      e.preventDefault();
      dispatch('close');
    }
  };

  const handleOverlayScroll = () => {
    if (!mainHeaderEl) return;
    const titlebarHeight = parseInt(
      getComputedStyle(document.documentElement).getPropertyValue('--titlebar-height') || '50',
      10,
    ) || 50;
    const headerRect = mainHeaderEl.getBoundingClientRect();
    showStickyHeader = headerRect.bottom <= titlebarHeight + 8;
  };

  scrollHost = overlayEl;
  if (scrollHost) {
    scrollHost.addEventListener('scroll', handleOverlayScroll, { passive: true });
  }
  handleOverlayScroll();
  setupInfiniteScroll();
  
  window.addEventListener('mouseup', handleMouseButton);
  
  return () => {
    window.removeEventListener('mouseup', handleMouseButton);
    if (scrollHost) {
      scrollHost.removeEventListener('scroll', handleOverlayScroll);
    }
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

    let response;
    if (category === 'trending') {
      response = await getTrending(type === 'all' ? 'all' : type, 'day', page);
    } else if (category === 'discover_by_genre' || category === 'discover_by_keyword') {
      const discoverParams = {
        page,
        sort_by: 'popularity.desc'
      };
      if (category === 'discover_by_genre') {
        discoverParams.with_genres = String(filterId ?? genre ?? '');
      } else {
        discoverParams.with_keywords = String(filterId ?? '');
      }

      if (type === 'movie') {
        response = await discoverMovies(discoverParams);
      } else if (type === 'tv') {
        response = await discoverTV(discoverParams);
      } else {
        const [movieResponse, tvResponse] = await Promise.all([
          discoverMovies(discoverParams),
          discoverTV(discoverParams)
        ]);

        const movieResults = (movieResponse?.results || []).map((item) => ({ ...item, media_type: 'movie' }));
        const tvResults = (tvResponse?.results || []).map((item) => ({ ...item, media_type: 'tv' }));
        response = {
          results: [...movieResults, ...tvResults].sort((a, b) => (b.popularity || 0) - (a.popularity || 0)),
          total_pages: Math.max(movieResponse?.total_pages || 1, tvResponse?.total_pages || 1),
        };
      }
    } else if (category === 'popular') {
      if (genre) {
        const params = { with_genres: String(genre), page, sort_by: 'popularity.desc' };
        response = type === 'movie' ? await discoverMovies(params) : await discoverTV(params);
      } else {
        response = type === 'movie' ? await getPopularMovies(page) : await getPopularTV(page);
      }
    } else if (category === 'top_rated') {
      response = type === 'movie' ? await getTopRatedMovies(page) : await getTopRatedTV(page);
    } else if (category === 'now_playing') {
      response = await getNowPlaying(page);
    }

    if (response?.results) {
      // Ensure media_type is set for all items
      const resultsWithType = response.results.map(item => {
        if (!item.media_type) {
          item.media_type = type === 'tv' ? 'tv' : 'movie';
        }
        return item;
      });
      
      if (page === 1) {
        items = resultsWithType;
      } else {
        const existingIds = new Set(items.map(item => item.id));
        const newItems = resultsWithType.filter(item => !existingIds.has(item.id));
        items = [...items, ...newItems];
      }

      resultsWithType.forEach((item) => {
        // no-op: color extraction removed
      });

      totalPages = response.total_pages || 1;
    }
  } catch (err) {
    error = err.message;
  }
  
  loading = false;
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

function handleCardEnter(_item) {}
function handleCardLeave(_item) {}

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

// getRatingColor replaced by getRatingClass in src/lib/utils/colorUtils.js
</script>

<div class="view-all-overlay" bind:this={overlayEl}>

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
      <div class="loading">Loading...</div>
    {:else if error}
      <div class="error">Error: {error}</div>
    {:else}
      <div class="view-all-grid">
        {#each items as item (item.id)}
          <!-- svelte-ignore a11y-click-events-have-key-events -->
          <!-- svelte-ignore a11y-no-static-element-interactions -->
          <div
            class="grid-card"
            on:click={() => openDetail(item)}
            on:mouseenter={() => handleCardEnter(item)}
            on:mouseleave={() => handleCardLeave(item)}
          >
            {#if item.poster_path}
              <img src={getImageUrl(item.poster_path, 'w342')} alt={item.title || item.name} loading="lazy" />
            {:else}
              <div class="no-poster">
                <i class="ri-film-line"></i>
              </div>
            {/if}
            
            {#if item.vote_average > 0}
              <div class="grid-card-rating">
                <span class="rating-badge {getRatingClass(item.vote_average)}">
                  {item.vote_average.toFixed(1)}
                </span>
              </div>
            {/if}
            
            <div class="grid-card-overlay">
              <div class="grid-card-info">
                <h3>{item.title || item.name}</h3>
                <div class="grid-card-meta">
                  <span class="year">{formatDate(item.release_date || item.first_air_date)}</span>
                  {#if item.vote_average > 0}
                    <span class="rating-badge {getRatingClass(item.vote_average)}">
                      {item.vote_average.toFixed(1)}
                    </span>
                  {/if}
                </div>
              </div>
              
              <div class="grid-card-actions">
                <button class="action-btn" title="Play" on:click={(e) => { e.stopPropagation(); }}>
                  <i class="ri-play-fill"></i>
                </button>
                <button 
                  class="action-btn" 
                  title={myListItems.has(`${item.id}-${item.media_type}`) ? 'Remove from List' : 'Add to List'}
                  on:click={(e) => toggleMyList(e, item)}
                >
                  <i class="{myListItems.has(`${item.id}-${item.media_type}`) ? 'ri-check-line' : 'ri-add-line'}"></i>
                </button>
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
