<script>
import { onDestroy } from 'svelte';
import { invoke } from '@tauri-apps/api/core';
import { getMovieRecommendations, getTVRecommendations, getImageUrl, getCorsImageUrl } from './tmdb.js';
import { getRatingColor } from './utils/colorUtils.js';
import { myListStore } from './stores/listStore.js';
import { watchProgressStore } from './stores/watchProgressStore.js';
import { watchHistoryStore } from './stores/watchHistoryStore.js';
import { fetchBrowseList, isEnglishFriendly, isReleased } from './browseLists.js';

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

function handleBackdropLoaded(imgId) {
  requestAnimationFrame(() => requestAnimationFrame(() => {
    backdropImages = backdropImages.map(i =>
      i.id === imgId && !i.visible ? { ...i, loaded: true, visible: true } : i
    );
  }));
}

// Svelte action: resolves the cached-image case where onload never re-fires in WebKit
function imageLoadAction(node, imgId) {
  if (node.complete && node.naturalWidth > 0) {
    handleBackdropLoaded(imgId);
  }
  return { destroy() {} };
}

const AUTO_ADVANCE_SECONDS = 9;
const MAX_SEEDS = 8;

let source = null; // 'personal' or 'trending'
let reasons = new Map();
let pool = [];
let loadToken = 0;
let loadTimer = null;
let lastSignature = null;

$: myList = $myListStore;
$: myListItems = new Set(myList.map(item => `${item.id}-${item.media_type}`));

$: currentItem = displayedRecommendations[currentIndex];
$: currentReason = currentItem ? reasons.get(`${currentItem.id}-${currentItem.media_type}`) : null;

// update backdrop with crossfade when currentItem changes
$: if (currentItem?.backdrop_path) {
  const newUrl = getImageUrl(currentItem.backdrop_path, 'w1280');
  // check if this url is already the latest in the array
  const latestImg = backdropImages[backdropImages.length - 1];
  if (!latestImg || latestImg.url !== newUrl) {
    // mark all existing images as not visible (fading out)
    backdropImages = backdropImages.map(img => ({ ...img, visible: false }));
    backdropIdCounter++;
    backdropImages = [...backdropImages, { url: newUrl, id: backdropIdCounter, visible: false, loaded: false }];
    // clean up old images after transition (keep max 2)
    setTimeout(() => {
      if (backdropImages.length > 2) {
        backdropImages = backdropImages.slice(-2);
      }
    }, 700);
  }
  extractColors(currentItem.backdrop_path);
}

// recent history and my list alternate so both shape the picks
$: seeds = buildSeeds(myList, $watchHistoryStore);
$: seedSignature = seeds.map(seed => seed.key).join(',');

// only reload while nothing personal is showing, so adding the current pick to my list doesn't reshuffle
$: if (seedSignature !== lastSignature) {
  lastSignature = seedSignature;
  if (source !== 'personal') scheduleLoad();
}

onDestroy(() => clearTimeout(loadTimer));

function buildSeeds(list, history) {
  const seen = new Set();
  const result = [];
  const longest = Math.max(list.length, history.length);
  for (let i = 0; i < longest && result.length < MAX_SEEDS; i++) {
    for (const [item, from] of [[history[i], 'history'], [list[i], 'list']]) {
      if (!item || result.length >= MAX_SEEDS) continue;
      const key = `${item.id}-${item.media_type}`;
      if (seen.has(key)) continue;
      seen.add(key);
      result.push({ key, item, from });
    }
  }
  return result;
}

function scheduleLoad() {
  clearTimeout(loadTimer);
  // the stores fill in one after another at startup, so wait for them to settle
  loadTimer = setTimeout(loadRecommendations, 300);
}

async function loadRecommendations() {
  const token = ++loadToken;
  loading = displayedRecommendations.length === 0;

  const personal = seeds.length > 0 ? await loadPersonal(seeds) : [];
  if (token !== loadToken) return;

  if (personal.length > 0) {
    source = 'personal';
    pool = personal;
    shuffleRecommendations();
  } else {
    const trending = await loadTrending();
    if (token !== loadToken) return;
    source = 'trending';
    pool = trending;
    displayedRecommendations = trending.slice(0, 10);
    currentIndex = 0;
  }
  loading = false;
}

async function loadPersonal(seedList) {
  const excluded = new Set([
    ...myList.map(item => `${item.id}-${item.media_type}`),
    ...$watchHistoryStore.map(item => `${item.id}-${item.media_type}`),
  ]);
  const scored = new Map();
  const nextReasons = new Map();

  await Promise.all(seedList.map(async (seed, seedIndex) => {
    try {
      const { item } = seed;
      const response = item.media_type === 'movie'
        ? await getMovieRecommendations(item.id)
        : await getTVRecommendations(item.id);
      const seedWeight = 1 - seedIndex * 0.08;

      (response?.results || []).forEach((rec, rank) => {
        const mediaType = rec.media_type || item.media_type;
        const key = `${rec.id}-${mediaType}`;
        if (excluded.has(key)) return;
        if (!rec.backdrop_path || !rec.overview || (rec.vote_count || 0) < 50 || !isReleased(rec)) return;
        // stay in english unless the seed itself is in the same language, like a korean drama in my list
        if (!isEnglishFriendly(rec) && rec.original_language !== item.original_language) return;

        const score = seedWeight / Math.sqrt(rank + 1);
        const entry = scored.get(key) || { item: { ...rec, media_type: mediaType }, score: 0, best: 0 };
        entry.score += score;
        if (score > entry.best) {
          entry.best = score;
          const seedTitle = item.title || item.name;
          nextReasons.set(key, seed.from === 'history' ? `Because you watched ${seedTitle}` : `Because ${seedTitle} is in your list`);
        }
        scored.set(key, entry);
      });
    } catch (err) {
      console.error('Error fetching recommendations:', err);
    }
  }));

  // titles several seeds agree on rise to the top, nudged by how well they are rated
  const ranked = [...scored.values()]
    .map(entry => ({ ...entry, score: entry.score * (0.6 + 0.4 * Math.min(entry.item.vote_average || 0, 9) / 9) }))
    .sort((a, b) => b.score - a.score)
    .slice(0, 30);

  reasons = nextReasons;
  return ranked;
}

async function loadTrending() {
  try {
    const { results } = await fetchBrowseList({ category: 'trending' });
    const picks = results.filter(item => item.backdrop_path && item.overview);
    reasons = new Map(picks.map(item => [`${item.id}-${item.media_type}`, 'Trending Today']));
    return picks;
  } catch (err) {
    console.error('Error fetching trending:', err);
    return [];
  }
}

// weighted sample without replacement, so strong picks show up more often but not always
function shuffleRecommendations() {
  if (source !== 'personal') {
    displayedRecommendations = [...pool].sort(() => Math.random() - 0.5).slice(0, 10);
  } else {
    displayedRecommendations = pool
      .map(entry => ({ item: entry.item, order: Math.random() ** (1 / Math.max(entry.score, 0.01)) }))
      .sort((a, b) => b.order - a.order)
      .slice(0, 10)
      .map(entry => entry.item);
  }
  currentIndex = 0;
}

async function extractColors(backdropPath) {
  try {
    const imageUrl = getCorsImageUrl(backdropPath, 'w300');
    const img = new Image();
    img.crossOrigin = 'Anonymous';
    img.src = imageUrl;
    await new Promise((resolve, reject) => {
      img.onload = resolve;
      img.onerror = reject;
      // WebKit may not re-fire onload for cached images; resolve immediately if already complete
      if (img.complete) {
        if (img.naturalWidth > 0) resolve();
        else reject(new Error('Image failed to load'));
      }
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
  if (!currentItem) return;
  const item = currentItem;
  const itemMediaType = item.media_type;
  const key = `${item.id}-${itemMediaType}`;
  if (playingItem) return; // already in-flight
  const progress = $watchProgressStore[key];
  playingItem = true;

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
      playingItem = false;
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

function toggleMyList(event) {
  event.stopPropagation();
  if (currentItem) {
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

{#if loading}
  <div class="recommendations-featured recommendations-skeleton" aria-hidden="true">
    <div class="featured-content">
      <div class="featured-header">
        <div class="rec-skeleton-poster rec-skeleton-pulse"></div>
        <div class="rec-skeleton-info">
          <div class="rec-skeleton-title rec-skeleton-pulse"></div>
          <div class="rec-skeleton-meta rec-skeleton-pulse"></div>
          <div class="rec-skeleton-tags">
            <span class="rec-skeleton-tag rec-skeleton-pulse"></span>
            <span class="rec-skeleton-tag rec-skeleton-pulse"></span>
            <span class="rec-skeleton-tag rec-skeleton-pulse"></span>
          </div>
          <div class="rec-skeleton-overview rec-skeleton-pulse"></div>
          <div class="rec-skeleton-overview short rec-skeleton-pulse"></div>
          <div class="rec-skeleton-actions">
            <span class="rec-skeleton-btn rec-skeleton-pulse"></span>
            <span class="rec-skeleton-btn rec-skeleton-pulse"></span>
          </div>
        </div>
      </div>
    </div>
  </div>
{:else if displayedRecommendations.length > 0 && currentItem}
  <div class="recommendations-featured" style="--backdrop-color: {backdropColor}; --prominent-color: {prominentColor}; --text-color: {textColor}">
    <div class="featured-backdrop">
      {#each backdropImages as img (img.id)}
        <img 
          src={img.url} 
          alt=""
          class="backdrop-img" 
          class:visible={img.visible}
          use:imageLoadAction={img.id}
          on:load={() => handleBackdropLoaded(img.id)}
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
              {#if currentReason}
                <span class="featured-reason">
                  <i class={source === 'personal' ? 'ri-sparkling-2-line' : 'ri-fire-line'}></i>
                  {currentReason}
                </span>
              {/if}
              <h1 class="detail-title">{currentItem.title || currentItem.name}</h1>

            <div class="detail-meta">
              <div class="rating-box" style="--rating-color: {getRatingColor(currentItem.vote_average)}" title="TMDB rating">
                <i class="ri-star-fill"></i><span class="rating-value">{formatRating(currentItem.vote_average)}</span><span class="rating-scale">/10</span>
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
              <span class="indicator-bar">
                {#if index === currentIndex}
                  <span class="indicator-fill" style:animation-duration="{AUTO_ADVANCE_SECONDS}s" on:animationend={() => navigateRecommendation('next')}></span>
                {/if}
              </span>
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
