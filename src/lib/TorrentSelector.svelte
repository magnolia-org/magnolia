<script>
    import { createEventDispatcher, onDestroy, onMount } from "svelte";
    import { fade, scale } from "svelte/transition";
    import { cubicOut } from 'svelte/easing';
    import { getTrackerPreference, setTrackerPreference } from "./stores/trackerPreference.js";
    import { open } from "@tauri-apps/plugin-dialog";
    import { readFile } from "@tauri-apps/plugin-fs";
    import { invoke } from "@tauri-apps/api/core";

    export let results = [];
    export let searchQuery = "";
    export let originalSearchQuery = ""; // The original auto-generated query
    export let loading = false;
    export let selectedTorrentName = "";
    export let isAnime = false;
    export let hasImdbId = false;
    export let isMovie = false;
    export let currentSeason = null;
    export let currentEpisode = null;

    const dispatch = createEventDispatcher();
    
    let trackerMode = 'auto';
    let selectedTrackers = [];
    export let isSelectingTorrent = false;

    // Installed tracker extensions: [{ id, label, isAnime }]
    let trackerExtensions = [];

    async function loadTrackerExtensions() {
        try {
            const storedPref = await getTrackerPreference();
            if (Array.isArray(storedPref) && storedPref.length > 0) {
                trackerMode = 'manual';
                selectedTrackers = storedPref;
            }
            const exts = await invoke('list_extensions');
            trackerExtensions = exts
                .filter(e => e.enabled && e.manifest.type === 'tracker')
                .map(e => ({
                    id: e.id,
                    label: e.manifest.tracker_name || e.manifest.name,
                    isAnime: !!e.manifest.is_anime,
                }));
            // Drop stored selections that point at removed/disabled extensions
            const known = new Set(trackerExtensions.map(t => t.id));
            const filtered = selectedTrackers.filter(t => known.has(t));
            if (filtered.length !== selectedTrackers.length) {
                selectedTrackers = filtered;
                if (selectedTrackers.length === 0) trackerMode = 'auto';
            }
        } catch (err) {
            console.error('failed to load tracker extensions:', err);
        }
    }

    onMount(loadTrackerExtensions);

    onDestroy(() => {
        if (researchTimeout) {
            clearTimeout(researchTimeout);
        }
    });

    let selectedBatch = "all";
    let selectedQuality = "all";
    let selectedEncode = "all";
    let selectedAudioCodec = "all";
    let prioritizeMatching = true;
    let sortBy = "relevance";
    let sortDirection = "desc";
    let searchFilter = "";
    let visibleCount = 50;
    
    let customMagnetLink = "";
    let magnetError = "";
    
    let editableSearchQuery = searchQuery;
    let isEditingQuery = false;
    let customTorrentExpanded = false;
    
    $: if (searchQuery && !isEditingQuery) {
        editableSearchQuery = searchQuery;
    }

    $: availableQualities = [...new Set(results.map(r => r.quality).filter(Boolean))].sort();
    $: availableEncodes = [...new Set(results.map(r => r.encode).filter(Boolean))].sort();
    $: availableAudioCodecs = [...new Set(results.map(r => r.audio_codec).filter(Boolean))].sort();

    $: filteredResults = results
        .filter(torrent => {
            if (searchFilter && !torrent.title.toLowerCase().includes(searchFilter.toLowerCase())) {
                return false;
            }
            if (selectedBatch === "batch" && !torrent.is_batch) return false;
            if (selectedBatch === "single" && torrent.is_batch) return false;
            if (selectedQuality !== "all" && torrent.quality !== selectedQuality) return false;
            if (selectedEncode !== "all" && torrent.encode !== selectedEncode) return false;
            if (selectedAudioCodec !== "all" && torrent.audio_codec !== selectedAudioCodec) return false;
            return true;
        })
        .sort((a, b) => {
            // ranking fields are scored by the backend search
            if (prioritizeMatching) {
                if (a.matches_episode !== b.matches_episode) return a.matches_episode ? -1 : 1;
                // for movies, torrents naming the release year come first
                if (a.has_release_year !== b.has_release_year) return a.has_release_year ? -1 : 1;
            }
            let comparison = 0;
            if (sortBy === "relevance") {
                comparison = b.relevance - a.relevance;
            } else if (sortBy === "seeds") {
                comparison = b.seeds - a.seeds;
            } else if (sortBy === "peers") {
                comparison = b.peers - a.peers;
            } else if (sortBy === "size") {
                comparison = b.size_bytes - a.size_bytes;
            } else if (sortBy === "name") {
                comparison = a.title.localeCompare(b.title);
            }
            return sortDirection === "desc" ? comparison : -comparison;
        });

    function selectTorrent(torrent) {
        if (loading || isSelectingTorrent) return;
        isSelectingTorrent = true;
        dispatch("select", torrent);
    }

    function cancelSelection() {
        isSelectingTorrent = false;
        dispatch("cancelSelection");
    }

    function close() {
        if (isSelectingTorrent) return;
        dispatch("close");
    }

    function resetFilters() {
        selectedBatch = "all";
        selectedQuality = "all";
        selectedEncode = "all";
        selectedAudioCodec = "all";
        sortBy = "relevance";
        sortDirection = "desc";
        searchFilter = "";
        visibleCount = 50;
    }

    // Reset visible window whenever the filtered list changes
    $: { filteredResults; visibleCount = 50; }

    function handleResultsScroll(e) {
        const el = e.currentTarget;
        if (el.scrollHeight - el.scrollTop - el.clientHeight < 200) {
            visibleCount = Math.min(visibleCount + 50, filteredResults.length);
        }
    }

    function toggleSort(column) {
        if (loading) return;
        if (sortBy === column) {
            sortDirection = sortDirection === "desc" ? "asc" : "desc";
        } else {
            sortBy = column;
            sortDirection = column === "name" ? "asc" : "desc";
        }
    }

    let researchTimeout;
    
    function selectAuto() {
        if (loading) return;
        trackerMode = 'auto';
        selectedTrackers = [];
        triggerResearch();
    }
    
    function toggleTracker(tracker) {
        if (loading) return;
        if (trackerMode === 'auto') {
            trackerMode = 'manual';
            selectedTrackers = [tracker];
        } else {
            const index = selectedTrackers.indexOf(tracker);
            if (index > -1) {
                selectedTrackers = selectedTrackers.filter(t => t !== tracker);
                if (selectedTrackers.length === 0) {
                    trackerMode = 'auto';
                }
            } else {
                selectedTrackers = [...selectedTrackers, tracker];
            }
        }
        triggerResearch();
    }
    
    function triggerResearch() {
        clearTimeout(researchTimeout);
        researchTimeout = setTimeout(() => {
            const trackerData = trackerMode === 'auto' ? [] : selectedTrackers;
            console.log("Dispatching research event with trackers:", trackerData);
            setTrackerPreference(trackerData);
            dispatch("research", { trackers: trackerData, query: editableSearchQuery });
        }, 800);
    }
    
    function handleSearchQueryKeydown(e) {
        if (e.key === 'Enter') {
            isEditingQuery = false;
            dispatch("research", { trackers: trackerMode === 'auto' ? [] : selectedTrackers, query: editableSearchQuery });
        } else if (e.key === 'Escape') {
            isEditingQuery = false;
            editableSearchQuery = searchQuery;
        }
    }
    
    function handleSearchQueryBlur() {
        isEditingQuery = false;
        if (editableSearchQuery !== searchQuery) {
            dispatch("research", { trackers: trackerMode === 'auto' ? [] : selectedTrackers, query: editableSearchQuery });
        }
    }
    
    function revertToOriginalQuery() {
        if (loading || !originalSearchQuery) return;
        editableSearchQuery = originalSearchQuery;
        dispatch("research", { trackers: trackerMode === 'auto' ? [] : selectedTrackers, query: originalSearchQuery, useImdb: true });
    }
    
    $: queryModified = originalSearchQuery && editableSearchQuery !== originalSearchQuery;
    
    // the backend validates the link and names it from its dn parameter
    async function parseMagnet(link) {
        return await invoke("parse_magnet_link", { link });
    }

    async function handleMagnetInput() {
        magnetError = "";
        const link = customMagnetLink;
        if (!link) return;
        try {
            await parseMagnet(link);
        } catch (err) {
            // ignore answers for text the user has already changed
            if (link === customMagnetLink) magnetError = String(err);
        }
    }

    async function submitCustomMagnet() {
        if (!customMagnetLink) return;
        try {
            dispatch("select", await parseMagnet(customMagnetLink));
        } catch (err) {
            magnetError = String(err);
        }
    }
    
    async function pickTorrentFile() {
        try {
            const selected = await open({
                multiple: false,
                filters: [{
                    name: 'Torrent Files',
                    extensions: ['torrent']
                }]
            });
            
            if (selected) {
                const fileData = await readFile(selected);
                const base64 = btoa(String.fromCharCode(...fileData));
                const fileName = selected.split(/[/\\]/).pop();
                const metadata = await invoke("parse_release_title", { title: fileName });

                dispatch("select", {
                    title: fileName,
                    torrent_file: base64,
                    size: "Unknown",
                    seeds: 0,
                    peers: 0,
                    provider: "file",
                    ...metadata,
                });
            }
        } catch (err) {
            console.error("Failed to open torrent file:", err);
            magnetError = "Failed to open file: " + err.message;
        }
    }
    
    // Compute which trackers are being used for display
    $: activeTrackerNames = (() => {
        if (trackerMode === 'auto') {
            // Auto mode - matches backend logic: anime media uses anime
            // trackers, everything else uses the general ones
            return trackerExtensions
                .filter(t => t.isAnime === isAnime)
                .map(t => t.label);
        }
        return selectedTrackers.map(t =>
            trackerExtensions.find(e => e.id === t)?.label ?? t
        );
    })();
</script>

<!-- svelte-ignore a11y-click-events-have-key-events -->
<!-- svelte-ignore a11y-no-static-element-interactions -->
<div class="modal-overlay" transition:fade|global={{ duration: 400, easing: cubicOut }} on:click={close}>
    <div class="modal-content" on:click|stopPropagation class:is-loading={loading} in:scale|global={{ start: 1.08, opacity: 1, duration: 400, easing: cubicOut }} out:scale|global={{ start: 1.08, opacity: 1, duration: 400, easing: cubicOut }}>
        <div class="modal-header">
            <div class="header-title">
                <h3>Select a Torrent</h3>
                {#if selectedTorrentName}
                    <span class="selected-torrent-name" title={selectedTorrentName}>
                        <i class="ri-checkbox-circle-fill"></i>
                        {selectedTorrentName.length > 60 ? selectedTorrentName.slice(0, 60) + '...' : selectedTorrentName}
                    </span>
                {/if}
            </div>
            <div class="tracker-selector">
                <span class="tracker-label">Tracker:</span>
                <div class="tracker-buttons">
                    <button class="tracker-btn" class:active={trackerMode === 'auto'} on:click={selectAuto} disabled={loading}>Auto</button>
                    {#each trackerExtensions as tracker (tracker.id)}
                        <button class="tracker-btn" class:active={selectedTrackers.includes(tracker.id)} on:click={() => toggleTracker(tracker.id)} disabled={loading}>{tracker.label}</button>
                    {/each}
                </div>
            </div>
        </div>

        <div class="search-info">
            <div class="search-query-wrapper">
                <span class="search-label">Results for:</span>
                <div class="search-query-input-wrapper">
                    <input 
                        type="text" 
                        class="search-query-input"
                        bind:value={editableSearchQuery}
                        on:focus={() => isEditingQuery = true}
                        on:blur={handleSearchQueryBlur}
                        on:keydown={handleSearchQueryKeydown}
                        disabled={loading}
                    />
                    {#if queryModified}
                        <button class="revert-query-btn" on:click={revertToOriginalQuery} disabled={loading} title="Revert to original search">
                            <i class="ri-arrow-go-back-line"></i>
                        </button>
                    {/if}
                </div>
                <span class="result-count">{filteredResults.length} of {results.length} results</span>
            </div>
        </div>

        {#if !loading && results.length > 0}
            <div class="filters-bar">
                <div class="filter-search-inline">
                    <i class="ri-filter-2-line"></i>
                    <input type="text" placeholder="Filter..." bind:value={searchFilter} class="filter-input" disabled={loading} />
                    {#if searchFilter}
                        <button class="clear-filter" on:click={() => searchFilter = ""}>
                            <i class="ri-close-line"></i>
                        </button>
                    {/if}
                </div>
                
                <div class="filter-group">
                    <span class="filter-label">Type:</span>
                    <div class="filter-options">
                        <button class="filter-chip" class:active={selectedBatch === 'all'} on:click={() => selectedBatch = 'all'}>All</button>
                        <button class="filter-chip" class:active={selectedBatch === 'single'} on:click={() => selectedBatch = 'single'}>Single</button>
                        <button class="filter-chip" class:active={selectedBatch === 'batch'} on:click={() => selectedBatch = 'batch'}>Batch</button>
                    </div>
                </div>

                {#if availableQualities.length > 0}
                    <div class="filter-group">
                        <span class="filter-label">Quality:</span>
                        <div class="filter-options">
                            <button class="filter-chip" class:active={selectedQuality === 'all'} on:click={() => selectedQuality = 'all'}>All</button>
                            {#each availableQualities as quality}
                                <button class="filter-chip" class:active={selectedQuality === quality} on:click={() => selectedQuality = quality}>{quality}</button>
                            {/each}
                        </div>
                    </div>
                {/if}

                {#if availableEncodes.length > 0}
                    <div class="filter-group">
                        <span class="filter-label">Encode:</span>
                        <div class="filter-options">
                            <button class="filter-chip" class:active={selectedEncode === 'all'} on:click={() => selectedEncode = 'all'}>All</button>
                            {#each availableEncodes as encode}
                                <button class="filter-chip" class:active={selectedEncode === encode} on:click={() => selectedEncode = encode}>{encode}</button>
                            {/each}
                        </div>
                    </div>
                {/if}

                {#if (currentSeason && currentEpisode) || isMovie}
                    <div class="filter-group">
                        <span class="filter-label">Priority:</span>
                        <div class="filter-options">
                            <button class="filter-chip" class:active={prioritizeMatching} on:click={() => prioritizeMatching = !prioritizeMatching} title="Push matching torrents to the top">
                                <i class="ri-arrow-up-line"></i>
                            </button>
                        </div>
                    </div>
                {/if}

                <button class="reset-btn" on:click={resetFilters} title="Reset filters">
                    <i class="ri-refresh-line"></i>
                </button>
            </div>
        {/if}

        <div class="results-list" on:scroll={handleResultsScroll}>
            {#if loading}
                <div class="loading-state">
                    <div class="spinner"></div>
                    <p>Searching {activeTrackerNames.join(', ')} for "{editableSearchQuery}"...</p>
                    <span class="loading-subtext">This may take a few seconds</span>
                </div>
            {:else if results.length === 0}
                <div class="empty-state">
                    <i class="ri-file-search-line"></i>
                    <p>No results found</p>
                </div>
            {:else if filteredResults.length === 0}
                <div class="empty-state">
                    <i class="ri-filter-line"></i>
                    <p>No results match the current filters</p>
                    <button class="reset-btn-alt" on:click={resetFilters}>Reset Filters</button>
                </div>
            {:else}
                <div class="table-header">
                    <!-- svelte-ignore a11y-click-events-have-key-events -->
                    <!-- svelte-ignore a11y-no-static-element-interactions -->
                    <div class="header-col col-name" on:click={() => toggleSort('name')}>
                        <span>NAME</span>
                        {#if sortBy === 'name'}<i class="ri-arrow-{sortDirection === 'asc' ? 'up' : 'down'}-s-line"></i>{/if}
                    </div>
                    <!-- svelte-ignore a11y-click-events-have-key-events -->
                    <!-- svelte-ignore a11y-no-static-element-interactions -->
                    <div class="header-col col-size" on:click={() => toggleSort('size')}>
                        <span>SIZE</span>
                        {#if sortBy === 'size'}<i class="ri-arrow-{sortDirection === 'asc' ? 'up' : 'down'}-s-line"></i>{/if}
                    </div>
                    <!-- svelte-ignore a11y-click-events-have-key-events -->
                    <!-- svelte-ignore a11y-no-static-element-interactions -->
                    <div class="header-col col-seeds" on:click={() => toggleSort('seeds')}>
                        <span>SEEDS</span>
                        {#if sortBy === 'seeds'}<i class="ri-arrow-{sortDirection === 'asc' ? 'up' : 'down'}-s-line"></i>{/if}
                    </div>
                    <!-- svelte-ignore a11y-click-events-have-key-events -->
                    <!-- svelte-ignore a11y-no-static-element-interactions -->
                    <div class="header-col col-peers" on:click={() => toggleSort('peers')}>
                        <span>PEERS</span>
                        {#if sortBy === 'peers'}<i class="ri-arrow-{sortDirection === 'asc' ? 'up' : 'down'}-s-line"></i>{/if}
                    </div>
                </div>
                <div class="table-body">
                    {#each filteredResults.slice(0, visibleCount) as torrent}
                        <div class="torrent-row" class:disabled={loading} class:matches-episode={torrentMatchesCurrentEpisode(torrent)} class:has-year={torrentHasReleaseYear(torrent)} on:click={() => selectTorrent(torrent)}>
                            <div class="col-name">
                                <div class="torrent-title">{torrent.title}</div>
                                {#if torrent.quality || torrent.encode || torrent.is_batch || torrent.season || torrent.episode || torrent.provider}
                                    <div class="metadata-tags">
                                        {#if torrent.provider}
                                            <span class="tag tag-provider">{torrent.provider}</span>
                                        {/if}
                                        {#if torrent.season && torrent.episode}
                                            <span class="tag tag-episode">S{torrent.season.toString().padStart(2, '0')}E{torrent.episode.toString().padStart(2, '0')}</span>
                                        {:else if torrent.season}
                                            <span class="tag tag-episode">Season {torrent.season}</span>
                                        {/if}
                                        {#if torrent.quality}
                                            <span class="tag tag-quality">{torrent.quality}</span>
                                        {/if}
                                        {#if torrent.encode}
                                            <span class="tag tag-encode">{torrent.encode}</span>
                                        {/if}
                                        {#if torrent.is_batch}
                                            <span class="tag tag-batch">BATCH</span>
                                        {/if}
                                    </div>
                                {/if}
                            </div>
                            <div class="col-size">{torrent.size}</div>
                            <div class="col-seeds {torrent.seeds >= 10 ? 'seeds-high' : torrent.seeds >= 3 ? 'seeds-med' : torrent.seeds > 0 ? 'seeds-low' : ''}">{torrent.seeds}</div>
                            <div class="col-peers">{torrent.peers}</div>
                        </div>
                    {/each}
                </div>
            {/if}
        </div>
        
        <!-- svelte-ignore a11y-click-events-have-key-events -->
        <!-- svelte-ignore a11y-no-static-element-interactions -->
        <div class="custom-torrent-section" class:expanded={customTorrentExpanded}>
            <div class="custom-torrent-inputs">
                <div class="magnet-input-wrapper">
                    <input 
                        type="text" 
                        placeholder="Paste magnet link here..." 
                        bind:value={customMagnetLink}
                        on:input={handleMagnetInput}
                        class="magnet-input"
                        class:error={magnetError}
                        disabled={loading}
                    />
                    <button class="submit-magnet-btn" on:click={submitCustomMagnet} disabled={loading || !customMagnetLink || magnetError}>
                        <i class="ri-arrow-right-line"></i>
                    </button>
                </div>
                {#if magnetError}
                    <span class="magnet-error">{magnetError}</span>
                {/if}
                <button class="pick-file-btn" on:click={pickTorrentFile} disabled={loading}>
                    <i class="ri-file-add-line"></i>
                    Pick .torrent file
                </button>
            </div>
            <div class="custom-torrent-header" on:click={() => customTorrentExpanded = !customTorrentExpanded}>
                <i class="ri-add-line"></i>
                <span>Or use own torrent...</span>
            </div>
        </div>
    </div>
    
    {#if isSelectingTorrent}
        <div class="selection-loading-overlay" transition:fade={{ duration: 150 }}>
            <div class="selection-loading-content">
                <div class="spinner"></div>
                <p>Loading torrent metadata...</p>
                <span class="loading-subtext">Please wait...</span>
                <button class="cancel-button" on:click={cancelSelection}>
                    <i class="ri-close-line"></i> Cancel
                </button>
            </div>
        </div>
    {/if}
</div>

<style>
  @import '../styles/torrent-selector.css';
</style>
