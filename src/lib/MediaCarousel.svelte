<script>
import { onMount, afterUpdate, onDestroy, createEventDispatcher } from 'svelte';
import { getTrending, getPopularMovies, getPopularTV, getTopRatedMovies, getTopRatedTV, getNowPlaying, discoverTV, getImageUrl } from './tmdb.js';
import { myListStore } from './stores/listStore.js';
import { getRatingClass } from './utils/colorUtils.js';

const dispatch = createEventDispatcher();

export let title = 'Section Title';
export let type = 'movie';
export let category = 'popular';
export let genre = null;
export let accentColor = '#6366f1';
export let customItems = null;
export let showClearButton = false;
export let hideViewAll = false;
export let isRecentlyWatched = false;
export let watchProgress = {};

let items = [];
let loading = true;
let error = null;
let carouselElement;
let showLeftArrow = false;
let showRightArrow = false;
let cardColors = {};

function getItemKey(item) {
  return `${item.id}-${item.media_type || type}`;
}

$: myListItems = new Set($myListStore.map(item => `${item.id}-${item.media_type}`));
$: {
  if (title === "My List") console.log('📺 My List carousel updated:', myListItems.size, 'items in store');
}
$: {
  if (myListItems && items.length > 0) {
    console.log(`🔄 ${title}: myListItems updated, size:`, myListItems.size);
  }
}
$: {
  if (isRecentlyWatched && watchProgress) {
    console.log('📊 watchProgress updated in carousel:', Object.keys(watchProgress).length, 'entries');
  }
}

$: if (customItems) {
  cardColors = {};
  items = customItems;
  loading = false;
  console.log('🔄 Custom items updated:', customItems.length, 'items');
  customItems.forEach(item => {
    const itemKey = getItemKey(item);
    if (isRecentlyWatched || title === 'My List') {
      cardColors[itemKey] = accentColor;
    } else if (item.poster_path) {
      extractDominantColor(itemKey, getImageUrl(item.poster_path, 'w92'));
    } else {
      cardColors[itemKey] = accentColor;
    }
  });
}

onMount(async () => {
  if (customItems) {
    return;
  }

  try {
    let response;
    if (category === 'trending') {
      response = await getTrending(type === 'all' ? 'all' : type, 'day');
    } else if (category === 'popular') {
      if (genre) {
        response = await discoverTV({ with_genres: genre, sort_by: 'popularity.desc' });
      } else if (type === 'movie') {
        response = await getPopularMovies();
      } else if (type === 'tv') {
        response = await getPopularTV();
      }
    } else if (category === 'top_rated') {
      if (genre) {
        response = await discoverTV({ with_genres: genre, sort_by: 'vote_average.desc', 'vote_count.gte': 100 });
      } else if (type === 'movie') {
        response = await getTopRatedMovies();
      } else if (type === 'tv') {
        response = await getTopRatedTV();
      }
    } else if (category === 'now_playing') {
      response = await getNowPlaying();
    }
    items = (response?.results || []).map(item => {
      // Ensure media_type is set if not already present
      if (!item.media_type) {
        item.media_type = type === 'tv' ? 'tv' : 'movie';
      }
      return item;
    });
    loading = false;

    items.forEach(item => {
      if (item.poster_path) {
        extractDominantColor(getItemKey(item), getImageUrl(item.poster_path, 'w92'));
      }
    });
  } catch (err) {
    console.error('Error fetching TMDB data:', err);
    error = err.message;
    loading = false;
  }
});

// Keep arrows in sync with window resizing so visibility toggles
// when the viewport changes (e.g. responsive layout).
function handleResize() {
  updateArrows();
}

onMount(() => {
  window.addEventListener('resize', handleResize);
});

onDestroy(() => {
  window.removeEventListener('resize', handleResize);
});

afterUpdate(() => {
updateArrows();
});

function formatRating(rating) {
return rating ? rating.toFixed(1) : 'N/A';
}

function formatDate(dateStr) {
if (!dateStr) return 'N/A';
const date = new Date(dateStr);
return date.getFullYear();
}

// getRatingColor was moved to `src/lib/utils/colorUtils.js`. Use getRatingClass to apply semantic classes.

function updateArrows() {
if (!carouselElement) return;
const scrollLeft = carouselElement.scrollLeft;
const scrollWidth = carouselElement.scrollWidth;
const clientWidth = carouselElement.clientWidth;
showLeftArrow = scrollLeft > 10;
showRightArrow = scrollLeft < scrollWidth - clientWidth - 10;
}

function scroll(direction) {
if (!carouselElement) return;
const scrollAmount = 400;
carouselElement.scrollBy({
left: direction === 'left' ? -scrollAmount : scrollAmount,
behavior: 'smooth'
});
setTimeout(updateArrows, 300);
}

async function extractDominantColor(itemKey, imageUrl) {
try {
const img = new Image();
img.crossOrigin = 'Anonymous';
img.src = imageUrl;
await new Promise((resolve, reject) => {
img.onload = resolve;
img.onerror = reject;
});

const canvas = document.createElement('canvas');
const ctx = canvas.getContext('2d');
canvas.width = img.width;
canvas.height = img.height;
ctx.drawImage(img, 0, 0);

const imageData = ctx.getImageData(0, 0, canvas.width, canvas.height).data;
let r = 0, g = 0, b = 0, count = 0;

// Sample pixels and calculate average to avoid very dark or light swatches
for (let i = 0; i < imageData.length; i += 4 * 4) {
const red = imageData[i];
const green = imageData[i + 1];
const blue = imageData[i + 2];
const brightness = (red + green + blue) / 3;

if (brightness > 40 && brightness < 180) {
r += red;
g += green;
b += blue;
count++;
}
}

if (count > 0) {
r = Math.floor(r / count);
g = Math.floor(g / count);
b = Math.floor(b / count);
const max = Math.max(r, g, b);
const min = Math.min(r, g, b);
const saturation = max === 0 ? 0 : (max - min) / max;
const boost = 1.5; // Increase saturation
r = Math.min(255, Math.floor(r + (r - min) * boost * saturation));
g = Math.min(255, Math.floor(g + (g - min) * boost * saturation));
b = Math.min(255, Math.floor(b + (b - min) * boost * saturation));
cardColors[itemKey] = `rgb(${r}, ${g}, ${b})`;
} else {
cardColors[itemKey] = accentColor;
}
cardColors = cardColors;
} catch (err) {
cardColors[itemKey] = accentColor;
cardColors = cardColors;
}
}

function openDetail(item) {
  // Ensure media_type is set before dispatching
  if (!item.media_type) {
    item.media_type = type === 'tv' ? 'tv' : 'movie';
  }
  
  // When opening detail (not quick play), strip episode tracking info
  // so it doesn't auto-navigate to a specific episode
  const cleanItem = { ...item };
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

function isInMyList(item) {
  const inList = myListItems.has(`${item.id}-${item.media_type}`);
  return inList;
}

function toggleMyList(event, item) {
  event.stopPropagation();
  // Ensure media_type is set before adding to list
  if (!item.media_type) {
    item.media_type = type === 'tv' ? 'tv' : 'movie';
  }
  console.log('🔘 Toggle button clicked for:', item.title || item.name);
  myListStore.toggleItem(item);
}

function handleQuickPlay(event, item) {
  event.stopPropagation();
  
  // Get saved progress
  const key = `${item.id}-${item.media_type}`;
  const progress = watchProgress[key];
  
  // Open detail and trigger autoplay with resume progress
  window.dispatchEvent(new CustomEvent('openMediaDetail', { 
    detail: { ...item, autoPlay: true, resumeProgress: progress } 
  }));
}

function handleRemoveFromHistory(event, item) {
  event.stopPropagation();
  dispatch('removeItem', { id: item.id, media_type: item.media_type });
}

function getProgressInfo(item) {
  // Check watchProgress prop first, then fall back to item data
  const key = `${item.id}-${item.media_type}`;
  const progress = watchProgress?.[key] || (item.current_season || item.currentSeason ? item : null);
  
  if (!progress) return null;
  
  // Handle both snake_case (from Rust) and camelCase (from JS)
  const season = progress.current_season || progress.currentSeason;
  const episode = progress.current_episode || progress.currentEpisode;
  const timestamp = progress.current_timestamp || progress.currentTimestamp;
  
  // For TV shows, show episode
  if ((item.media_type === 'tv' || item.first_air_date) && season && episode) {
    const s = season.toString().padStart(2, '0');
    const e = episode.toString().padStart(2, '0');
    
    // If there's also a timestamp, show it too
    if (timestamp && timestamp > 60) {
      const minutes = Math.floor(timestamp / 60);
      const seconds = Math.floor(timestamp % 60);
      return `S${s}E${e} · ${minutes}:${seconds.toString().padStart(2, '0')}`;
    }
    
    return `S${s}E${e}`;
  }
  
  // For movies, show timestamp
  if (timestamp && timestamp > 60) {
    const totalMinutes = Math.floor(timestamp / 60);
    const seconds = Math.floor(timestamp % 60);
    const hours = Math.floor(totalMinutes / 60);
    const minutes = totalMinutes % 60;
    
    if (hours > 0) {
      return `${hours}h ${minutes}m`;
    }
    return `${totalMinutes}:${seconds.toString().padStart(2, '0')}`;
  }
  
  return null;
}

function getProgressPercentage(item) {
  const key = `${item.id}-${item.media_type}`;
  const timestamp = item.current_timestamp || item.currentTimestamp;
  const progress = watchProgress?.[key] || (timestamp ? item : null);
  
  if (!progress || !progress.currentTimestamp || !progress.duration) return 0;
  
  return Math.min(100, Math.max(0, (progress.currentTimestamp / progress.duration) * 100));
}

function handleViewAll() {
  const detail = {
    title,
    type,
    category,
    genre,
    customItems
  };
  window.dispatchEvent(new CustomEvent('viewAll', { detail }));
}
</script>

<div class="carousel-section">
<div class="section-header">
<h2 class="section-title">{title}</h2>
<div class="header-actions">
  {#if showClearButton}
    <button class="btn-standard clear-btn" on:click={() => dispatch('clear')} title="Clear history">
      <i class="ri-delete-bin-line"></i>
      Clear
    </button>
  {/if}
  {#if !hideViewAll}
    <button class="btn-standard view-all" on:click={handleViewAll}>View All →</button>
  {/if}
</div>
</div>
{#if loading}
<div class="loading">Loading...</div>
{:else if error}
<div class="error">Error: {error}</div>
{:else}
    <div class="carousel-container" class:show-left-gradient={showLeftArrow} class:show-right-gradient={showRightArrow}>
      <button class="carousel-arrow left" class:visible={showLeftArrow} on:click={() => scroll('left')} aria-label="Scroll left">
        <i class="ri-arrow-left-s-line"></i>
      </button>
      <div class="carousel" bind:this={carouselElement} on:scroll={updateArrows}>
{#each items as item, index (`${item.id}-${item.media_type}-${index}`)}
<!-- svelte-ignore a11y-click-events-have-key-events -->
<!-- svelte-ignore a11y-no-static-element-interactions -->
<div class="media-card" style="--card-accent: {cardColors[getItemKey(item)] || accentColor}" on:click={() => openDetail(item)}>
{#if item.poster_path}
<img class="media-poster" src={getImageUrl(item.poster_path, 'w500')} alt={item.title || item.name} loading="lazy" />
{#if isRecentlyWatched}
  {@const progressInfo = getProgressInfo(item)}
  {#if progressInfo}
    <div class="progress-badge">{progressInfo}</div>
  {/if}
{/if}
{/if}
<div class="media-content">
<div class="media-info">
<h3 class="media-title">{item.title || item.name || 'Unknown'}</h3>
<div class="media-meta">
<span>{formatDate(item.release_date || item.first_air_date)}</span>
<span class="rating-badge {getRatingClass(item.vote_average)}">
{formatRating(item.vote_average)}
</span>
</div>
</div>
<div class="media-actions">
<button class="action-btn" title="Play" on:click={(e) => handleQuickPlay(e, item)}>
<i class="ri-play-fill"></i>
</button>
{#if isRecentlyWatched}
<button 
  class="action-btn" 
  title="Remove from Recently Watched"
  on:click={(e) => handleRemoveFromHistory(e, item)}
>
<i class="ri-close-line"></i>
</button>
{:else}
<button 
  class="action-btn" 
  title={myListItems.has(`${item.id}-${item.media_type}`) ? "Remove from List" : "Add to List"}
  on:click={(e) => toggleMyList(e, item)}
>
<i class="{myListItems.has(`${item.id}-${item.media_type}`) ? 'ri-check-line' : 'ri-add-line'}"></i>
</button>
{/if}
</div>
</div>
</div>
{/each}
</div>
<button class="carousel-arrow right" class:visible={showRightArrow} on:click={() => scroll('right')} aria-label="Scroll right">
<i class="ri-arrow-right-s-line"></i>
</button>
</div>
{/if}
</div>
