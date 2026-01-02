<script>
    import { createEventDispatcher, onDestroy } from "svelte";
    import { fade, scale } from "svelte/transition";
    import { getTrackerPreference, setTrackerPreference } from "./stores/watchHistoryStore.js";
    import { open } from "@tauri-apps/plugin-dialog";
    import { readFile } from "@tauri-apps/plugin-fs";

    export let results = [];
    export let searchQuery = "";
    export let originalSearchQuery = ""; // The original auto-generated query
    export let loading = false;
    export let selectedTorrentName = "";
    export let isAnime = false;
    export let hasImdbId = false;
    export let isTVShow = false;
    export let currentSeason = null;
    export let currentEpisode = null;

    const dispatch = createEventDispatcher();
    
    let trackerMode = 'auto';
    let selectedTrackers = [];
    let isSelectingTorrent = false;
    
    const storedPref = getTrackerPreference();
    if (Array.isArray(storedPref) && storedPref.length > 0) {
        trackerMode = 'manual';
        selectedTrackers = storedPref;
    } else {
        trackerMode = 'auto';
    }
    
    onDestroy(() => {
        if (researchTimeout) {
            clearTimeout(researchTimeout);
        }
    });

    let selectedBatch = "all";
    let selectedQuality = "all";
    let selectedEncode = "all";
    let selectedAudioCodec = "all";
    let hideIncompatible = true;
    let prioritizeMatching = true;
    let sortBy = "relevance";
    let sortDirection = "desc";
    let searchFilter = "";
    
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

    function isWebCompatible(codec) {
        if (!codec) return true;
        const compatible = ['AAC', 'MP3', 'Opus', 'Vorbis', 'FLAC'];
        return compatible.includes(codec);
    }
    
    function torrentMatchesCurrentEpisode(torrent) {
        if (!currentSeason || !currentEpisode) return false;
        
        // Check if torrent has explicit season/episode info
        if (torrent.season && torrent.episode) {
            return torrent.season === currentSeason && torrent.episode === currentEpisode;
        }
        
        // Check title for S01E05 or similar patterns
        const title = torrent.title.toUpperCase();
        const s = currentSeason.toString().padStart(2, '0');
        const e = currentEpisode.toString().padStart(2, '0');
        
        if (title.includes(`S${s}E${e}`) || title.includes(`${currentSeason}X${e}`)) {
            return true;
        }
        
        // Check for batch torrents that cover this season
        if (torrent.is_batch || title.includes('BATCH') || title.includes('SEASON')) {
            // If torrent has season info, check if it matches
            if (torrent.season) {
                return torrent.season === currentSeason;
            }
            // Check title for season number
            const seasonMatch = title.match(/S(\d{1,2})/i) || title.match(/SEASON[\s._-]*(\d{1,2})/i);
            if (seasonMatch) {
                const torrentSeason = parseInt(seasonMatch[1]);
                return torrentSeason === currentSeason;
            }
        }
        
        return false;
    }

    $: filteredResults = results
        .filter(torrent => {
            if (searchFilter && !torrent.title.toLowerCase().includes(searchFilter.toLowerCase())) {
                return false;
            }
            if (selectedBatch === "batch" && !torrent.is_batch) return false;
            if (selectedBatch === "single" && torrent.is_batch) return false;
            if (selectedQuality !== "all" && torrent.quality !== selectedQuality) return false;
            if (selectedEncode !== "all" && torrent.encode !== selectedEncode) return false;
            if (hideIncompatible && !isWebCompatible(torrent.audio_codec)) return false;
            if (selectedAudioCodec !== "all" && torrent.audio_codec !== selectedAudioCodec) return false;
            return true;
        })
        .sort((a, b) => {
            // Prioritize matching torrents first if enabled
            if (prioritizeMatching) {
                const aMatches = torrentMatchesCurrentEpisode(a);
                const bMatches = torrentMatchesCurrentEpisode(b);
                if (aMatches && !bMatches) return -1;
                if (!aMatches && bMatches) return 1;
            }
            let comparison = 0;
            if (sortBy === "relevance") {
                const maxSeeds = Math.max(...filteredResults.map(t => t.seeds));
                const seedThreshold = maxSeeds * 0.1;
                
                const aSeedPenalty = a.seeds < seedThreshold ? 0.5 : 1;
                const bSeedPenalty = b.seeds < seedThreshold ? 0.5 : 1;
                
                // Detect batch torrents (for TV shows, prioritize torrents with "batch", "season", or "complete" in title)
                const isBatchA = isTVShow && /\b(batch|season|complete|s\d{2}|1080p.*(?:season|complete))\b/i.test(a.title);
                const isBatchB = isTVShow && /\b(batch|season|complete|s\d{2}|1080p.*(?:season|complete))\b/i.test(b.title);
                const batchBonusA = isBatchA ? 1.5 : 1;
                const batchBonusB = isBatchB ? 1.5 : 1;
                
                const aPopularity = ((a.seeds * 2) + a.peers + (parseSize(a.size) / (1024**3)) * 0.1) * aSeedPenalty * batchBonusA;
                const bPopularity = ((b.seeds * 2) + b.peers + (parseSize(b.size) / (1024**3)) * 0.1) * bSeedPenalty * batchBonusB;
                comparison = bPopularity - aPopularity;
            } else if (sortBy === "seeds") {
                comparison = b.seeds - a.seeds;
            } else if (sortBy === "peers") {
                comparison = b.peers - a.peers;
            } else if (sortBy === "size") {
                comparison = parseSize(b.size) - parseSize(a.size);
            } else if (sortBy === "name") {
                comparison = a.title.localeCompare(b.title);
            }
            return sortDirection === "desc" ? comparison : -comparison;
        });

    function parseSize(sizeStr) {
        const units = { 'B': 1, 'KB': 1024, 'KiB': 1024, 'MB': 1024**2, 'MiB': 1024**2, 'GB': 1024**3, 'GiB': 1024**3, 'TB': 1024**4, 'TiB': 1024**4 };
        const match = sizeStr.match(/^([\d.]+)\s*(\w+)$/);
        if (!match) return 0;
        return parseFloat(match[1]) * (units[match[2]] || 1);
    }

    function selectTorrent(torrent) {
        if (loading || isSelectingTorrent) return;
        isSelectingTorrent = true;
        dispatch("select", torrent);
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
        hideIncompatible = true;
        sortBy = "relevance";
        sortDirection = "desc";
        searchFilter = "";
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
        }, 300);
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
    
    function isValidMagnet(link) {
        return /^magnet:\?xt=urn:[a-z0-9]+:[a-z0-9]{32,}/i.test(link);
    }
    
    function handleMagnetInput() {
        magnetError = "";
        if (customMagnetLink && !isValidMagnet(customMagnetLink)) {
            magnetError = "Invalid magnet link format";
        }
    }
    
    function parseTorrentMetadata(title) {
        const seasonMatch = title.match(/S(\d{1,2})|Season\s*(\d{1,2})/i);
        const episodeMatch = title.match(/S\d{1,2}E(\d+)|E(\d+)|Episode\s*(\d+)/i);
        const qualityMatch = title.match(/(\d{3,4}p|4K|2160p|1080p|720p|480p)/i);
        const encodeMatch = title.match(/(x264|x265|H\.?264|H\.?265|HEVC|AVC|VP9|AV1)/i);
        const batchMatch = title.match(/(batch|complete|\d+-\d+|S\d+E\d+-E?\d+)/i);
        
        const season = seasonMatch ? parseInt(seasonMatch[1] || seasonMatch[2]) : null;
        const episode = episodeMatch ? parseInt(episodeMatch[1] || episodeMatch[2] || episodeMatch[3]) : null;
        const quality = qualityMatch ? qualityMatch[1].toUpperCase() : null;
        const encode = encodeMatch ? encodeMatch[1].toUpperCase() : null;
        let is_batch = !!batchMatch;
        
        // If has season but no episode, likely a batch
        if (season && !episode) {
            is_batch = true;
        }
        
        return { season, episode, quality, encode, is_batch };
    }
    
    function extractTitleFromMagnet(magnetLink) {
        const dnMatch = magnetLink.match(/dn=([^&]+)/);
        if (dnMatch) {
            return decodeURIComponent(dnMatch[1].replace(/\+/g, ' '));
        }
        return "Custom Magnet Link";
    }
    
    function submitCustomMagnet() {
        if (!customMagnetLink) return;
        if (!isValidMagnet(customMagnetLink)) {
            magnetError = "Invalid magnet link format";
            return;
        }
        
        const title = extractTitleFromMagnet(customMagnetLink);
        const metadata = parseTorrentMetadata(title);
        
        dispatch("select", {
            title: title,
            magnet_link: customMagnetLink,
            size: "Unknown",
            seeds: 0,
            peers: 0,
            provider: "custom",
            ...metadata,
        });
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
                const metadata = parseTorrentMetadata(fileName);
                
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
            // Auto mode - matches backend logic
            if (isAnime) {
                return ['Nyaa'];
            } else {
                // Regular TV/movies: limetorrents, thepiratebay, and eztv if imdb available
                const names = ['LimeTorrents', 'TPB'];
                if (hasImdbId) {
                    names.push('EZTV');
                }
                return names;
            }
        }
        return selectedTrackers.map(t => {
            switch(t) {
                case 'nyaa': return 'Nyaa';
                case 'limetorrents': return 'LimeTorrents';
                case 'thepiratebay': return 'TPB';
                case 'eztv': return 'EZTV';
                default: return t;
            }
        });
    })();
</script>

<!-- svelte-ignore a11y-click-events-have-key-events -->
<!-- svelte-ignore a11y-no-static-element-interactions -->
<div class="modal-overlay" transition:fade on:click={close}>
    <div class="modal-content" transition:scale on:click|stopPropagation class:is-loading={loading}>
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
                    <button class="tracker-btn" class:active={selectedTrackers.includes('nyaa')} on:click={() => toggleTracker('nyaa')} disabled={loading}>Nyaa</button>
                    <button class="tracker-btn" class:active={selectedTrackers.includes('limetorrents')} on:click={() => toggleTracker('limetorrents')} disabled={loading}>Lime</button>
                    <button class="tracker-btn" class:active={selectedTrackers.includes('thepiratebay')} on:click={() => toggleTracker('thepiratebay')} disabled={loading}>TPB</button>
                    <button class="tracker-btn" class:active={selectedTrackers.includes('eztv')} on:click={() => toggleTracker('eztv')} disabled={loading}>EZTV</button>
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

                <div class="filter-group">
                    <span class="filter-label">Compat:</span>
                    <div class="filter-options">
                        <button class="filter-chip" class:active={hideIncompatible} on:click={() => hideIncompatible = !hideIncompatible} title="Hide torrents with incompatible audio codecs (AC3, DTS, TrueHD, etc.)">
                            <i class="ri-{hideIncompatible ? 'eye-off' : 'eye'}-line"></i>
                        </button>
                    </div>
                </div>
                
                {#if currentSeason && currentEpisode}
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

        <div class="results-list">
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
                    {#each filteredResults as torrent}
                        <div class="torrent-row" class:disabled={loading} class:matches-episode={torrentMatchesCurrentEpisode(torrent)} on:click={() => selectTorrent(torrent)}>
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
                            <div class="col-seeds {torrent.seeds > 0 ? 'has-seeds' : ''}">{torrent.seeds}</div>
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
            </div>
        </div>
    {/if}
</div>

<style>
  @import '../styles/torrent-selector.css';
</style>
