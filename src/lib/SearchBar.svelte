<script>
  import { searchMulti, getImageUrl } from './tmdb.js';
  import { onMount } from 'svelte';
  import { getRatingClass } from './utils/colorUtils.js';
  
  export let searchActive = false;
  let searchQuery = '';
  let searchResults = [];
  let searching = false;
  let searchTimeout;
  let searchWrapper;
  let searchInput;
  let selectedIndex = -1;
  let recentSearches = [];
  let showRecent = false;
  let searchResultsContainer;
  let searchPage = 1;
  let searchTotalPages = 1;
  let loadingMore = false;
  
  $: searchActive = searchQuery.length > 0 || showRecent;
  
  $: if (selectedIndex >= 0 && searchResultsContainer) {
    scrollToSelected();
  }
  
  function scrollToSelected() {
    const items = searchResultsContainer?.querySelectorAll('.search-result-item');
    if (items && items[selectedIndex]) {
      const item = items[selectedIndex];
      const container = searchResultsContainer;
      const itemTop = item.offsetTop;
      const itemBottom = itemTop + item.offsetHeight;
      const containerTop = container.scrollTop;
      const containerBottom = containerTop + container.clientHeight;
      
      if (itemTop < containerTop) {
        container.scrollTop = itemTop;
      } else if (itemBottom > containerBottom) {
        container.scrollTop = itemBottom - container.clientHeight;
      }
    }
  }
  
  onMount(() => {
    const saved = localStorage.getItem('recentSearches');
    if (saved) {
      recentSearches = JSON.parse(saved);
    }

    const handleKeyDown = (e) => {
      if ((e.ctrlKey || e.metaKey) && (e.key === 'k' || e.key === 'K')) {
        e.preventDefault();
        try { searchInput?.focus({ preventScroll: true }); }
        catch (err) { searchInput?.focus(); }
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  });
  
  async function performSearch() {
    if (searchQuery.length < 2) {
      searchResults = [];
      selectedIndex = -1;
      searchPage = 1;
      searchTotalPages = 1;
      return;
    }
    
    searching = true;
    try {
      const response = await searchMulti(searchQuery, 1);
      searchResults = (response.results || [])
        .filter(item => item.media_type !== 'person')
        .sort((a, b) => (b.popularity || 0) - (a.popularity || 0));
      searchPage = 1;
      searchTotalPages = response.total_pages || 1;
      selectedIndex = -1;
    } catch (err) {
      console.error('Search error:', err);
      searchResults = [];
    }
    searching = false;
  }

  async function loadMoreSearchResults() {
    if (loadingMore || searchPage >= searchTotalPages || searchQuery.length < 2) return;
    loadingMore = true;
    try {
      const nextPage = searchPage + 1;
      const response = await searchMulti(searchQuery, nextPage);
      const newResults = (response.results || [])
        .filter(item => item.media_type !== 'person');
      searchResults = [...searchResults, ...newResults];
      searchPage = nextPage;
    } catch (err) {
      console.error('Load more search error:', err);
    }
    loadingMore = false;
  }

  function handleSearchScroll(e) {
    const el = e.currentTarget;
    if (el.scrollHeight - el.scrollTop - el.clientHeight < 150) {
      loadMoreSearchResults();
    }
  }
  
  function handleInput() {
    showRecent = false;
    clearTimeout(searchTimeout);
    searchTimeout = setTimeout(performSearch, 150);
  }

  function handleMouseDown(e) {
    // Prevent the browser from scrolling the page to the input when clicked
    // (especially important for sticky/floating header layouts)
    e.preventDefault();
    // Use preventScroll when available to avoid scrolling the page
    try {
      searchInput?.focus({ preventScroll: true });
    } catch (err) {
      // Older browsers may not support preventScroll - fallback to normal focus
      searchInput?.focus();
    }
  }

  function handleFocus() {
    if (searchQuery.length === 0 && recentSearches.length > 0) {
      showRecent = true;
    } else if (searchQuery.length >= 2) {
      performSearch();
    }
  }

  function handleKeyDown(e) {
    const itemCount = searchResults.length || recentSearches.length;
    
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      selectedIndex = Math.min(selectedIndex + 1, itemCount - 1);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      selectedIndex = Math.max(selectedIndex - 1, -1);
    } else if (e.key === 'Enter' && selectedIndex >= 0) {
      e.preventDefault();
      const item = showRecent ? recentSearches[selectedIndex] : searchResults[selectedIndex];
      // Blur the input so it's deselected when opening via keyboard
      try { searchInput?.blur(); } catch (err) { /* ignore */ }
      openDetail(item);
    } else if (e.key === 'Escape') {
      searchActive = false;
      showRecent = false;
      searchInput?.blur();
    }
  }
  
  function handleClickOutside(event) {
    if (searchWrapper && !searchWrapper.contains(event.target)) {
      searchActive = false;
      showRecent = false;
    }
  }
  
  function getMediaType(item) {
    if (item.media_type === 'movie') return 'Movie';
    if (item.media_type === 'tv') return 'TV Show';
    return 'Unknown';
  }
  
  function formatRating(rating) {
    return rating ? rating.toFixed(1) : 'N/A';
  }
  
  // getRatingColor replaced by getRatingClass in src/lib/utils/colorUtils.js

  function openDetail(item) {
    const exists = recentSearches.findIndex(s => s.id === item.id && s.media_type === item.media_type);
    if (exists >= 0) {
      recentSearches.splice(exists, 1);
    }
    recentSearches.unshift(item);
    recentSearches = recentSearches.slice(0, 5);
    localStorage.setItem('recentSearches', JSON.stringify(recentSearches));

    // window opens detail; to avoid layout shift when selecting via keyboard
    // delay hiding the results until navigation works in the parent. This avoids
    // the 'floating' results bar and content jump when openMediaDetail triggers.
    window.dispatchEvent(new CustomEvent('openMediaDetail', { detail: item }));
    // Hide search after the event has been handled by listeners (most notably App.svelte)
    // We use requestAnimationFrame twice to allow DOM updates to flush and avoid a measured
    // reflow that could cause the search results to float.
    requestAnimationFrame(() => requestAnimationFrame(() => {
      searchActive = false;
      showRecent = false;
      selectedIndex = -1;
    }));
  }

  function clearRecentSearches() {
    recentSearches = [];
    localStorage.removeItem('recentSearches');
    showRecent = false;
  }
</script>

<svelte:window on:click={handleClickOutside} />

<div class="search-wrapper" class:expanded={searchActive} bind:this={searchWrapper}>
  <div class="search-container">
    <input 
      type="text" 
      placeholder="Search..." 
      bind:value={searchQuery}
      bind:this={searchInput}
      on:input={handleInput}
      on:focus={handleFocus}
      on:keydown={handleKeyDown}
      on:mousedown={handleMouseDown}
      class="search-input"
    />
    {#if !searchActive && searchQuery.length === 0}
      <div class="search-shortcut">
        <kbd>Ctrl</kbd>+<kbd>K</kbd>
      </div>
    {/if}
  </div>
  <div class="search-results" bind:this={searchResultsContainer} class:visible={searchActive} aria-hidden={!searchActive} on:scroll={handleSearchScroll}>
      {#if showRecent && recentSearches.length > 0}
        <div class="results-header">
          <span>Recent Searches</span>
          <button class="clear-recent-btn" on:click={clearRecentSearches} title="Clear recent searches">
            <i class="ri-close-line"></i>
            Clear
          </button>
        </div>
        {#each recentSearches as result, index}
          <!-- svelte-ignore a11y-click-events-have-key-events -->
          <!-- svelte-ignore a11y-no-static-element-interactions -->
          <div 
            class="search-result-item" 
            class:selected={selectedIndex === index}
            on:click={() => openDetail(result)}
          >
            {#if result.poster_path || result.profile_path}
              <img src={getImageUrl(result.poster_path || result.profile_path, 'w92')} alt={result.title || result.name} />
            {:else}
              <div class="no-image">?</div>
            {/if}
            <div class="result-info">
              <div class="result-title">{result.title || result.name}</div>
              <div class="result-meta">
                <span>{getMediaType(result)}</span>
                {#if result.release_date || result.first_air_date}
                  <span class="separator">•</span>
                  <span>{(result.release_date || result.first_air_date).split('-')[0]}</span>
                {/if}
                {#if result.vote_average}
                  <span class="rating-badge {getRatingClass(result.vote_average)}">
                    {formatRating(result.vote_average)}
                  </span>
                {/if}
              </div>
            </div>
          </div>
        {/each}
      {:else if searching}
        <div class="results-placeholder">Searching...</div>
      {:else if searchResults.length > 0}
        {#each searchResults as result, index}
          <!-- svelte-ignore a11y-click-events-have-key-events -->
          <!-- svelte-ignore a11y-no-static-element-interactions -->
          <div 
            class="search-result-item" 
            class:selected={selectedIndex === index}
            on:click={() => openDetail(result)}
          >
            {#if result.poster_path || result.profile_path}
              <img src={getImageUrl(result.poster_path || result.profile_path, 'w92')} alt={result.title || result.name} />
            {:else}
              <div class="no-image">?</div>
            {/if}
            <div class="result-info">
              <div class="result-title">{result.title || result.name}</div>
              <div class="result-meta">
                <span>{getMediaType(result)}</span>
                {#if result.release_date || result.first_air_date}
                  <span class="separator">•</span>
                  <span>{(result.release_date || result.first_air_date).split('-')[0]}</span>
                {/if}
                {#if result.vote_average}
                  <span class="rating-badge {getRatingClass(result.vote_average)}">
                    {formatRating(result.vote_average)}
                  </span>
                {/if}
              </div>
            </div>
          </div>
        {/each}
      {:else if searchQuery.length >= 2}
        <div class="results-placeholder">No results found</div>
      {/if}
      {#if loadingMore}
        <div class="results-placeholder">Loading...</div>
      {/if}
    </div>
</div>
