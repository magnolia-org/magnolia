<script>
import { onMount } from 'svelte';
import { invoke } from '@tauri-apps/api/core';
import { getMovieRecommendations, getTVRecommendations, getImageUrl } from './tmdb.js';
import { getRatingColor } from './utils/colorUtils.js';
import { myListStore } from './stores/listStore.js';
import { watchProgressStore } from './stores/watchProgressStore.js';

let allRecommendations = [];
let displayedRecommendations = [];
let currentIndex = 0;
let loading = true;
let backdropColor = '#1a1a1a';
let prominentColor = '#1a1a1a';
let textColor = '#ffffff';
let isTransitioning = false;
let slideDirection = 'right';
let playingItem = false;

// Backdrop crossfade management - use array to keep both images in DOM
let backdropImages = []; // Array of {url, id, visible}
let backdropIdCounter = 0;

$: myList = $myListStore;
$: myListItems = new Set(myList.map(item => `${item.id}-${item.media_type}`));
$: {
  if (currentItem) {
    const inList = myListItems.has(`${currentItem.id}-${currentItem.media_type}`);
    console.log('🎬 Recommendations: Button state for', currentItem.title || currentItem.name, ':', inList);
  }
}

$: currentItem = displayedRecommendations[currentIndex];

// Update backdrop with crossfade when currentItem changes
$: if (currentItem?.backdrop_path) {
  const newUrl = getImageUrl(currentItem.backdrop_path, 'original');
  // Check if this URL is already the latest in the array
  const latestImg = backdropImages[backdropImages.length - 1];
  if (!latestImg || latestImg.url !== newUrl) {
    // Mark all existing images as not visible (fading out)
    backdropImages = backdropImages.map(img => ({ ...img, visible: false }));
    // Add new image
    backdropIdCounter++;
    backdropImages = [...backdropImages, { url: newUrl, id: backdropIdCounter, visible: false, loaded: false }];
    // Clean up old images after transition (keep max 2)
    setTimeout(() => {
      if (backdropImages.length > 2) {
        backdropImages = backdropImages.slice(-2);
      }
    }, 700);
  }
  extractColors(currentItem.backdrop_path);
}

$: if (myList) {
  loadRecommendations();
}

onMount(() => {
  loadRecommendations();
});


async function loadRecommendations() {
  if (myList.length === 0) {
    allRecommendations = [];
    displayedRecommendations = [];
    loading = false;
    return;
  }

  loading = true;
  const recsMap = new Map();
  const myListIds = new Set(myList.map(item => `${item.id}-${item.media_type}`));

  const fetchPromises = myList.slice(0, 5).map(async (item) => {
    try {
      const isMovie = item.media_type === 'movie';
      const recommendationsResponse = isMovie 
        ? await getMovieRecommendations(item.id) 
        : await getTVRecommendations(item.id);

      const results = recommendationsResponse?.results || [];

      results.forEach(rec => {
        const key = `${rec.id}-${item.media_type}`;
        if (myListIds.has(key)) return;
        
        if (!rec.media_type) {
          rec.media_type = item.media_type;
        }
        
        if (!recsMap.has(key) || recsMap.get(key).vote_average < rec.vote_average) {
          recsMap.set(key, rec);
        }
      });
    } catch (err) {
      console.error('Error fetching recommendations:', err);
    }
  });

  await Promise.all(fetchPromises);

  allRecommendations = Array.from(recsMap.values())
    .filter(rec => rec.vote_average > 0)
    .sort((a, b) => b.vote_average - a.vote_average);

  shuffleRecommendations();
  loading = false;
}

function shuffleRecommendations() {
  const shuffled = [...allRecommendations].sort(() => Math.random() - 0.5);
  displayedRecommendations = shuffled.slice(0, 10);
  currentIndex = 0;
}

async function extractColors(backdropPath) {
  try {
    const imageUrl = getImageUrl(backdropPath, 'w300');
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
    
  // Build a histogram of mid-range brightness pixels to find a dominant accent color
    const colorMap = {};
    for (let i = 0; i < imageData.length; i += 4 * 10) {
      const r = imageData[i];
      const g = imageData[i + 1];
      const b = imageData[i + 2];
      const brightness = (r + g + b) / 3;
      
      if (brightness > 40 && brightness < 200) {
        const colorKey = `${Math.floor(r/20)*20},${Math.floor(g/20)*20},${Math.floor(b/20)*20}`;
        colorMap[colorKey] = (colorMap[colorKey] || 0) + 1;
      }
    }
    
    let maxCount = 0;
    let dominantColor = null;
    for (const [color, count] of Object.entries(colorMap)) {
      if (count > maxCount) {
        maxCount = count;
        dominantColor = color;
      }
    }
    
    if (dominantColor) {
      const [r, g, b] = dominantColor.split(',').map(Number);
      prominentColor = `rgb(${r}, ${g}, ${b})`;
      
      textColor = '#ffffff';
    }
    
  // Derive a muted backdrop by averaging pixels within a limited brightness range
    let br = 0, bg = 0, bb = 0, count = 0;
    for (let i = 0; i < imageData.length; i += 4 * 10) {
      const red = imageData[i];
      const green = imageData[i + 1];
      const blue = imageData[i + 2];
      const brightness = (red + green + blue) / 3;

      if (brightness > 30 && brightness < 180) {
        br += red;
        bg += green;
        bb += blue;
        count++;
      }
    }

    if (count > 0) {
      br = Math.floor((br / count) * 0.3);
      bg = Math.floor((bg / count) * 0.3);
      bb = Math.floor((bb / count) * 0.3);
      backdropColor = `rgb(${br}, ${bg}, ${bb})`;
    }
  } catch (err) {
    console.error('Color extraction failed:', err);
  }
}

function rgbToHex(r, g, b) {
  return '#' + [r, g, b].map(x => {
    const hex = x.toString(16);
    return hex.length === 1 ? '0' + hex : hex;
  }).join('');
}

function navigateRecommendation(direction) {
  if (isTransitioning) return;
  isTransitioning = true;
  slideDirection = direction === 'next' ? 'right' : 'left';
  
  if (direction === 'next') {
    currentIndex = (currentIndex + 1) % displayedRecommendations.length;
  } else {
    currentIndex = (currentIndex - 1 + displayedRecommendations.length) % displayedRecommendations.length;
  }
  
  setTimeout(() => {
    isTransitioning = false;
  }, 450);
}

function goToIndex(index) {
  if (isTransitioning || index === currentIndex) return;
  isTransitioning = true;
  slideDirection = index > currentIndex ? 'right' : 'left';
  currentIndex = index;
  setTimeout(() => {
    isTransitioning = false;
  }, 450);
}

function openDetail() {
  if (currentItem) {
    window.dispatchEvent(new CustomEvent('openMediaDetail', { detail: currentItem }));
  }
}

async function handleQuickPlay() {
  if (!currentItem || playingItem) return;
  const item = currentItem;
  const itemMediaType = item.media_type;
  const key = `${item.id}-${itemMediaType}`;
  const progress = $watchProgressStore[key];
  const isMovie = itemMediaType === 'movie';
  playingItem = true;

  let targetSeason = 0;
  let targetEpisode = 0;
  if (!isMovie) {
    if (progress?.currentSeason && progress?.currentEpisode) {
      targetSeason = progress.currentSeason;
      targetEpisode = progress.currentEpisode;
    } else {
      targetSeason = 1;
      targetEpisode = 1;
    }
  }

  try {
    const saved = await invoke('get_saved_selection', {
      showId: Number(item.id),
      season: targetSeason,
      episode: targetEpisode
    });

    if (saved && saved.magnet_link) {
      const handleId = await invoke('add_torrent', { magnetOrUrl: saved.magnet_link });
      const mediaTitle = item.title || item.name || '';
      const playerTitle = isMovie
        ? mediaTitle
        : `${mediaTitle} - S${targetSeason}E${targetEpisode}`;
      let initialTimestamp = 0;
      if (isMovie && progress?.currentTimestamp) {
        initialTimestamp = progress.currentTimestamp;
      } else if (!isMovie && progress?.currentSeason === targetSeason && progress?.currentEpisode === targetEpisode) {
        initialTimestamp = progress.currentTimestamp || 0;
      }
      playingItem = false;
      window.dispatchEvent(new CustomEvent('openVideoPlayer', {
        detail: {
          src: null,
          title: playerTitle,
          metadata: item,
          handleId,
          fileIndex: saved.file_index,
          magnetLink: saved.magnet_link,
          initialTimestamp,
          mediaId: item.id,
          mediaType: itemMediaType,
          seasonNum: isMovie ? null : targetSeason,
          episodeNum: isMovie ? null : targetEpisode,
        }
      }));
      return;
    }
  } catch (err) {
    console.warn('Quick play: saved selection check failed, falling back to detail view', err);
  }

  playingItem = false;
  window.dispatchEvent(new CustomEvent('openMediaDetail', {
    detail: { ...item, autoPlay: true, resumeProgress: progress }
  }));
}

function isInMyList(item) {
  const inList = myListItems.has(`${item.id}-${item.media_type}`);
  return inList;
}

function toggleMyList(event) {
  event.stopPropagation();
  if (currentItem) {
    console.log('🎬 Recommendations: Toggle for:', currentItem.title || currentItem.name);
    myListStore.toggleItem(currentItem);
  }
}

function formatRating(rating) {
  return rating ? rating.toFixed(1) : 'N/A';
}

function formatDate(dateStr) {
  if (!dateStr) return 'N/A';
  return new Date(dateStr).toLocaleDateString('en-US', { year: 'numeric', month: 'long', day: 'numeric' });
}

function formatRuntime(minutes) {
  if (!minutes) return '';
  const hours = Math.floor(minutes / 60);
  const mins = minutes % 60;
  return `${hours}h ${mins}m`;
}

/* Rating color logic moved to src/lib/utils/colorUtils.js */

function getGenres(item) {
  if (item.genre_ids && item.genre_ids.length > 0) {
    const genreMap = {
      28: 'Action', 12: 'Adventure', 16: 'Animation', 35: 'Comedy', 80: 'Crime',
      99: 'Documentary', 18: 'Drama', 10751: 'Family', 14: 'Fantasy', 36: 'History',
      27: 'Horror', 10402: 'Music', 9648: 'Mystery', 10749: 'Romance', 878: 'Sci-Fi',
      10770: 'TV Movie', 53: 'Thriller', 10752: 'War', 37: 'Western', 10759: 'Action & Adventure',
      10762: 'Kids', 10763: 'News', 10764: 'Reality', 10765: 'Sci-Fi & Fantasy', 10766: 'Soap',
      10767: 'Talk', 10768: 'War & Politics'
    };
    return item.genre_ids.slice(0, 3).map(id => genreMap[id] || '').filter(Boolean);
  }
  return [];
}
</script>

{#if myList.length === 0}
  <div class="recommendations-empty">
    <div class="empty-content">
      <i class="ri-heart-line"></i>
      <h2>Start Building Your Collection</h2>
      <p>Add movies and shows to My List to get personalized recommendations</p>
    </div>
  </div>
{:else if !loading && displayedRecommendations.length > 0 && currentItem}
  <div class="recommendations-featured" style="--backdrop-color: {backdropColor}; --prominent-color: {prominentColor}; --text-color: {textColor}">
    <div class="featured-backdrop">
      {#each backdropImages as img (img.id)}
        <img 
          src={img.url} 
          alt=""
          class="backdrop-img" 
          class:visible={img.visible}
          on:load={() => {
            // Mark this image as loaded and visible
            backdropImages = backdropImages.map(i => 
              i.id === img.id ? { ...i, loaded: true, visible: true } : i
            );
          }} 
        />
      {/each}
    </div>

    <button class="shuffle-btn-top" on:click={shuffleRecommendations} title="Shuffle">
      <i class="ri-refresh-line"></i>
    </button>
    
    <div class="featured-content">
      {#key currentItem.id}
      <div class="featured-header">
        <div class="detail-poster" style="animation: {slideDirection === 'right' ? 'fadeCardSlide3dRight' : 'fadeCardSlide3dLeft'} 0.5s ease; transform-style: preserve-3d;">
          {#if currentItem.poster_path}
            <img src={getImageUrl(currentItem.poster_path, 'w500')} alt={currentItem.title || currentItem.name} />
          {/if}
        </div>

        <div class="detail-info-wrapper" style="animation: {slideDirection === 'right' ? 'fadeSlide3dRight' : 'fadeSlide3dLeft'} 0.5s ease; transform-style: preserve-3d;">
          <div class="detail-info">
              <h1 class="detail-title">{currentItem.title || currentItem.name}</h1>

            <div class="detail-meta">
              <div class="rating-box" style="background: {getRatingColor(currentItem.vote_average)}">
                {formatRating(currentItem.vote_average)}
              </div>
              <span>{formatDate(currentItem.release_date || currentItem.first_air_date)}</span>
            </div>

            {#if getGenres(currentItem).length > 0}
              <div class="detail-genres">
                {#each getGenres(currentItem) as genre}
                  <span class="genre-tag">{genre}</span>
                {/each}
              </div>
            {/if}

            {#if currentItem.overview}
                <p class="detail-overview">{currentItem.overview}</p>
            {/if}

            <div class="detail-actions">
              <button class="btn-standard primary" class:loading={playingItem} disabled={playingItem} on:click={handleQuickPlay}>
                {#if playingItem}
                  <i class="ri-loader-4-line spin"></i>
                {:else}
                  <i class="ri-play-fill"></i>
                {/if}
                Play
              </button>
              <button class="btn-standard" on:click={toggleMyList}>
                <i class="{myListItems.has(`${currentItem.id}-${currentItem.media_type}`) ? 'ri-check-line' : 'ri-add-line'}"></i>
                {myListItems.has(`${currentItem.id}-${currentItem.media_type}`) ? 'In My List' : 'My List'}
              </button>
              <button class="btn-standard" on:click={openDetail} title="More Info">
                <i class="ri-information-line"></i>
              </button>
            </div>
          </div>
        </div>
      </div>
      {/key}

      <div class="recommendations-nav">
        {#if displayedRecommendations.length > 1}
        <button class="nav-arrow" on:click={() => navigateRecommendation('prev')} disabled={isTransitioning} aria-label="Previous recommendation">
          <i class="ri-arrow-left-s-line"></i>
        </button>
        
        <div class="recommendations-indicators">
          {#each displayedRecommendations as item, index}
            <button 
              class="indicator" 
              class:active={index === currentIndex}
              on:click={() => goToIndex(index)}
              disabled={isTransitioning}
              aria-label="Go to recommendation {index + 1}"
            >
              <span class="indicator-bar"></span>
            </button>
          {/each}
        </div>

        <button class="nav-arrow" on:click={() => navigateRecommendation('next')} disabled={isTransitioning} aria-label="Next recommendation">
          <i class="ri-arrow-right-s-line"></i>
        </button>
        {/if}
      </div>
    </div>
  </div>
{/if}

<!-- styles migrated to src/styles/main.css -->
