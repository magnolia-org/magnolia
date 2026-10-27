<script>
    import { createEventDispatcher, onDestroy, onMount } from "svelte";
    import { fade, scale, slide } from "svelte/transition";
    import { cubicOut } from 'svelte/easing';
    import { getTrackerPreference, setTrackerPreference } from "./stores/trackerPreference.js";
    import { open } from "@tauri-apps/plugin-dialog";
    import { readFile } from "@tauri-apps/plugin-fs";
    import { invoke } from "@tauri-apps/api/core";
    import { resultKey } from "./torrentSearch.js";

    export let results = [];
    // tracker jobs of the running search: { tracker_id, tracker, kind, query, status, count, error }
    export let jobs = [];
    export let searchNote = "";
    // where the episode falls in anilist's numbering, for anime
    export let anime = null;
    export let searchQuery = "";
    export let originalSearchQuery = ""; // the original auto-generated query
    export let loading = false;
    export let selectedTorrentName = "";
    export let isAnime = false;
    export let isMovie = false;
    export let mediaTitle = "";
    export let releaseYear = null;
    export let currentSeason = null;
    export let currentEpisode = null;
    export let isSelectingTorrent = false;

    const dispatch = createEventDispatcher();

    const RESOLUTIONS = ["8K", "4K", "1440p", "1080p", "720p", "SD"];
    const PAGE_SIZE = 25;

    let trackerMode = 'auto';
    let selectedTrackers = [];

    // installed tracker extensions: [{ id, label, isAnime }]
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
            // drop stored selections that point at removed or disabled extensions
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

    onDestroy(() => clearTimeout(researchTimeout));

    let selectedResolution = "all";
    let hdrOnly = false;
    let subsOnly = false;
    let hideLowQuality = true;
    let scopeFilter = "all";
    let sortBy = "best";
    let sortOpen = false;
    let searchFilter = "";
    let expanded = {};
    let collapsedMismatch = true;

    let customMagnetLink = "";
    let magnetError = "";

    let editableSearchQuery = searchQuery;
    let isEditingQuery = false;
    let customTorrentExpanded = false;

    $: if (searchQuery && !isEditingQuery) {
        editableSearchQuery = searchQuery;
    }

    $: isEpisodeSearch = !isMovie && currentSeason > 0 && currentEpisode > 0;
    const pad = (n) => String(n).padStart(2, "0");
    $: contextLabel = isMovie
        ? [mediaTitle, releaseYear].filter(Boolean).join(" ")
        : isEpisodeSearch ? `${mediaTitle} S${pad(currentSeason)}E${pad(currentEpisode)}` : mediaTitle;

    // per tracker progress, summed over its passes
    $: trackerProgress = Object.values(jobs.reduce((acc, job) => {
        const t = acc[job.tracker_id] ||= { id: job.tracker_id, label: job.tracker, total: 0, finished: 0, count: 0, errors: [], queries: [] };
        t.total += 1;
        t.queries.push(job.query);
        if (job.status !== "running") t.finished += 1;
        t.count += job.count || 0;
        if (job.error) t.errors.push(`${job.query}: ${job.error}`);
        return acc;
    }, {}));
    $: finishedJobs = jobs.filter(j => j.status !== "running").length;
    $: progress = jobs.length ? finishedJobs / jobs.length : 0;

    $: lowQualityCount = results.filter(r => r.low_quality).length;
    $: baseResults = results.filter(r => !(hideLowQuality && r.low_quality));
    $: resolutionCounts = RESOLUTIONS
        .map(res => ({ res, count: baseResults.filter(r => r.resolution === res).length }))
        .filter(r => r.count > 0);
    $: hdrCount = baseResults.filter(r => r.hdr?.length).length;
    $: subsCount = baseResults.filter(r => hasReadableSubs(r)).length;

    function hasReadableSubs(r) {
        return r.subtitles === "english" || r.subtitles === "multi" || r.subtitles === "hardcoded";
    }

    function isPack(r) {
        return r.scope === "season" || r.scope === "seasons" || r.scope === "complete" || r.scope === "episodes";
    }

    const sorters = {
        best: (a, b) => b.relevance - a.relevance,
        seeds: (a, b) => b.seeds - a.seeds || b.relevance - a.relevance,
        largest: (a, b) => b.size_bytes - a.size_bytes,
        smallest: (a, b) => (a.size_bytes || Infinity) - (b.size_bytes || Infinity),
        newest: (a, b) => (b.uploaded || 0) - (a.uploaded || 0),
    };

    const SORT_OPTIONS = [
        { id: "best", label: "Best match", icon: "ri-sparkling-line" },
        { id: "seeds", label: "Most seeders", icon: "ri-arrow-up-circle-line" },
        { id: "newest", label: "Newest", icon: "ri-time-line" },
        { id: "largest", label: "Largest", icon: "ri-hard-drive-3-line" },
        { id: "smallest", label: "Smallest", icon: "ri-file-reduce-line" },
    ];
    $: sortLabel = SORT_OPTIONS.find(o => o.id === sortBy)?.label;

    function pickSort(id) {
        sortBy = id;
        sortOpen = false;
    }

    $: filteredResults = baseResults
        .filter(r => {
            if (searchFilter && !r.title.toLowerCase().includes(searchFilter.toLowerCase())) return false;
            if (selectedResolution !== "all" && r.resolution !== selectedResolution) return false;
            if (hdrOnly && !r.hdr?.length) return false;
            if (subsOnly && !hasReadableSubs(r)) return false;
            if (scopeFilter === "single" && isPack(r)) return false;
            if (scopeFilter === "pack" && !isPack(r)) return false;
            return true;
        })
        .sort(sorters[sortBy]);

    $: sections = buildSections(filteredResults, isMovie, isEpisodeSearch);
    // the top of the most relevant section, so the badge is the first thing seen
    $: bestResult = sortBy === "best" && sections[0] && sections[0].id !== "mismatch" ? sections[0].items[0] : null;
    $: bestResultKey = bestResult ? resultKey(bestResult) : null;

    function buildSections(list, movie, episodeSearch) {
        const by = (kind) => list.filter(r => r.match_kind === kind);
        if (movie) {
            return [
                { id: "exact", title: "Best matches", items: by("exact") },
                { id: "unknown", title: "Other releases", items: by("unknown") },
                { id: "mismatch", title: "Probably not this movie", items: by("mismatch"), collapsible: true },
            ].filter(s => s.items.length);
        }
        if (!episodeSearch) {
            return [{ id: "all", title: "Results", items: list }].filter(s => s.items.length);
        }
        return [
            { id: "exact", title: `Episode ${currentEpisode}`, items: by("exact") },
            { id: "pack", title: "Packs with this episode", items: by("pack") },
            { id: "unknown", title: "Other results", items: by("unknown") },
            { id: "mismatch", title: "Other episodes and seasons", items: by("mismatch"), collapsible: true },
        ].filter(s => s.items.length);
    }

    function scopeLabel(r) {
        const s = r.season;
        switch (r.scope) {
            case "episode":
                if (s && r.episode) return `S${pad(s)}E${pad(r.episode)}`;
                return r.episode ? `Episode ${r.episode}` : "Episode";
            case "episodes":
                return `${s ? `S${pad(s)} ` : ""}Episodes ${r.episode}-${r.episode_end}`;
            case "season":
                return s ? `Season ${s} pack` : "Season pack";
            case "seasons":
                return `Seasons ${r.season}-${r.season_end}`;
            case "complete":
                return "Complete series";
            default:
                return isMovie ? "Movie" : "";
        }
    }

    const SUB_LABELS = {
        english: { label: "English subs", icon: "ri-closed-captioning-line", tone: "good" },
        multi: { label: "Multi subs", icon: "ri-closed-captioning-line", tone: "good" },
        hardcoded: { label: "Hardsubs", icon: "ri-closed-captioning-fill", tone: "warn", title: "Subtitles are burned into the video and can't be turned off" },
        foreign: { label: "Foreign subs", icon: "ri-closed-captioning-line", tone: "warn", title: "Subtitles in a language other than English" },
        none: { label: "No subs", icon: "ri-forbid-line", tone: "warn", title: "A raw release without subtitles" },
    };

    function audioLabel(r) {
        const parts = [r.audio_codec, r.audio_channels].filter(Boolean);
        if (r.atmos) parts.push("Atmos");
        return parts.join(" ");
    }

    function videoLabel(r) {
        const parts = [r.video_codec];
        if (r.bit_depth && r.bit_depth !== 8) parts.push(`${r.bit_depth}-bit`);
        return parts.filter(Boolean).join(" ");
    }

    function languageLabel(r) {
        if (r.dual_audio) return "Dual audio";
        if (r.multi_audio) return "Multi audio";
        if (r.languages?.length > 2) return `${r.languages.length} languages`;
        if (r.languages?.length) return r.languages.join(", ");
        return r.dubbed ? "Dubbed" : "";
    }

    function age(uploaded) {
        if (!uploaded) return "";
        const days = Math.floor((Date.now() / 1000 - uploaded) / 86400);
        if (days < 1) return "today";
        if (days < 30) return `${days}d ago`;
        if (days < 365) return `${Math.floor(days / 30)}mo ago`;
        return `${Math.floor(days / 365)}y ago`;
    }

    function health(seeds) {
        if (seeds >= 50) return "great";
        if (seeds >= 10) return "good";
        if (seeds >= 3) return "ok";
        if (seeds > 0) return "low";
        return "dead";
    }

    function resolutionClass(res) {
        return res ? `res-${res.toLowerCase()}` : "res-unknown";
    }

    function showMore(id) {
        expanded = { ...expanded, [id]: (expanded[id] || PAGE_SIZE) + PAGE_SIZE * 2 };
    }

    // a new search starts with fresh section paging
    $: if (loading && results.length === 0) expanded = {};

    function selectTorrent(torrent) {
        if (isSelectingTorrent) return;
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

    function handleKeydown(e) {
        if (e.key !== "Escape" || isEditingQuery) return;
        if (sortOpen) sortOpen = false;
        else close();
    }

    function resetFilters() {
        selectedResolution = "all";
        hdrOnly = false;
        subsOnly = false;
        scopeFilter = "all";
        sortBy = "best";
        searchFilter = "";
        expanded = {};
    }

    let researchTimeout;

    function currentTrackers() {
        return trackerMode === 'auto' ? [] : selectedTrackers;
    }

    function selectAuto() {
        trackerMode = 'auto';
        selectedTrackers = [];
        triggerResearch();
    }

    function toggleTracker(tracker) {
        if (trackerMode === 'auto') {
            trackerMode = 'manual';
            selectedTrackers = [tracker];
        } else if (selectedTrackers.includes(tracker)) {
            selectedTrackers = selectedTrackers.filter(t => t !== tracker);
            if (selectedTrackers.length === 0) trackerMode = 'auto';
        } else {
            selectedTrackers = [...selectedTrackers, tracker];
        }
        triggerResearch();
    }

    // a quick run of tracker toggles becomes one search
    function triggerResearch() {
        clearTimeout(researchTimeout);
        researchTimeout = setTimeout(() => {
            setTrackerPreference(currentTrackers());
            dispatch("research", { trackers: currentTrackers(), query: editableSearchQuery });
        }, 400);
    }

    function handleSearchQueryKeydown(e) {
        if (e.key === 'Enter') {
            isEditingQuery = false;
            e.currentTarget.blur();
            dispatch("research", { trackers: currentTrackers(), query: editableSearchQuery });
        } else if (e.key === 'Escape') {
            isEditingQuery = false;
            editableSearchQuery = searchQuery;
            e.currentTarget.blur();
        }
    }

    function handleSearchQueryBlur() {
        if (!isEditingQuery) return;
        isEditingQuery = false;
        if (editableSearchQuery.trim() && editableSearchQuery !== searchQuery) {
            dispatch("research", { trackers: currentTrackers(), query: editableSearchQuery });
        }
    }

    function revertToOriginalQuery() {
        if (!originalSearchQuery) return;
        editableSearchQuery = originalSearchQuery;
        dispatch("research", { trackers: currentTrackers(), query: originalSearchQuery, useImdb: true });
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

    // what auto mode searches, matching the backend: anime trackers for anime, general ones otherwise
    $: activeTrackerNames = trackerMode === 'auto'
        ? trackerExtensions.filter(t => t.isAnime === isAnime).map(t => t.label)
        : selectedTrackers.map(t => trackerExtensions.find(e => e.id === t)?.label ?? t);
</script>

<svelte:window on:keydown={handleKeydown} on:click={() => sortOpen = false} />

<!-- svelte-ignore a11y-click-events-have-key-events -->
<!-- svelte-ignore a11y-no-static-element-interactions -->
<div class="modal-overlay" transition:fade|global={{ duration: 300, easing: cubicOut }} on:click={close}>
    <div class="modal-content" on:click|stopPropagation in:scale|global={{ start: 1.04, opacity: 1, duration: 300, easing: cubicOut }} out:scale|global={{ start: 1.04, opacity: 1, duration: 300, easing: cubicOut }}>
        <header class="modal-header">
            <div class="header-text">
                <h3>Choose a source</h3>
                {#if contextLabel}
                    <span class="header-context">
                        {contextLabel}
                        {#if anime}
                            <span class="anime-match" title="Episode numbering from AniList, which is how anime releases are named">
                                <i class="ri-links-line"></i>
                                {anime.title}, episode {anime.episode}{#if anime.absolute_episode !== anime.episode} (#{anime.absolute_episode} overall){/if}
                            </span>
                        {/if}
                    </span>
                {/if}
            </div>
            <button class="icon-btn close-btn" on:click={close} title="Close" aria-label="Close">
                <i class="ri-close-line"></i>
            </button>
        </header>

        {#if selectedTorrentName}
            <div class="previous-source" title={selectedTorrentName}>
                <i class="ri-history-line"></i>
                <span>Last used: {selectedTorrentName}</span>
            </div>
        {/if}

        <div class="search-row">
            <div class="query-box" class:focused={isEditingQuery}>
                <i class="ri-search-line"></i>
                <input
                    type="text"
                    class="query-input"
                    bind:value={editableSearchQuery}
                    on:focus={() => isEditingQuery = true}
                    on:blur={handleSearchQueryBlur}
                    on:keydown={handleSearchQueryKeydown}
                    placeholder="Search trackers..."
                    spellcheck="false"
                />
                {#if queryModified}
                    <button class="icon-btn" on:click={revertToOriginalQuery} title="Back to the original search" aria-label="Revert search">
                        <i class="ri-arrow-go-back-line"></i>
                    </button>
                {/if}
            </div>
            <div class="tracker-buttons" role="group" aria-label="Sources">
                <button class="tracker-btn" class:active={trackerMode === 'auto'} on:click={selectAuto} title="Pick trackers for this title automatically">Auto</button>
                {#each trackerExtensions as tracker (tracker.id)}
                    <button class="tracker-btn" class:active={selectedTrackers.includes(tracker.id)} on:click={() => toggleTracker(tracker.id)}>{tracker.label}</button>
                {/each}
            </div>
        </div>

        <div class="progress-row">
            {#each trackerProgress as t (t.id)}
                <span
                    class="progress-chip"
                    class:running={t.finished < t.total}
                    class:failed={t.finished === t.total && t.errors.length === t.total}
                    title={[`Searched: ${t.queries.join(", ")}`, ...t.errors].join("\n")}
                >
                    {#if t.finished < t.total}
                        <span class="mini-spinner"></span>
                    {:else if t.errors.length === t.total}
                        <i class="ri-error-warning-line"></i>
                    {:else}
                        <i class="ri-check-line"></i>
                    {/if}
                    {t.label}
                    <span class="chip-count">{t.finished < t.total && t.count === 0 ? `${t.finished}/${t.total}` : t.count}</span>
                </span>
            {/each}
            {#if searchNote && loading}
                <span class="progress-note">{searchNote}</span>
            {/if}
        </div>
        <div class="progress-bar" class:active={loading}>
            <div class="progress-fill" style="width: {loading ? Math.max(progress * 100, 8) : 100}%"></div>
        </div>

        {#if results.length > 0}
            <div class="filters-bar">
                <div class="segmented" role="group" aria-label="Resolution">
                    <button class:active={selectedResolution === 'all'} on:click={() => selectedResolution = 'all'}>All</button>
                    {#each resolutionCounts as { res, count } (res)}
                        <button class:active={selectedResolution === res} on:click={() => selectedResolution = selectedResolution === res ? 'all' : res}>
                            {res}<span class="seg-count">{count}</span>
                        </button>
                    {/each}
                </div>

                {#if !isMovie}
                    <div class="segmented" role="group" aria-label="Type">
                        <button class:active={scopeFilter === 'all'} on:click={() => scopeFilter = 'all'}>All</button>
                        <button class:active={scopeFilter === 'single'} on:click={() => scopeFilter = 'single'}>Episodes</button>
                        <button class:active={scopeFilter === 'pack'} on:click={() => scopeFilter = 'pack'}>Packs</button>
                    </div>
                {/if}

                {#if hdrCount > 0}
                    <button class="toggle-chip" class:active={hdrOnly} on:click={() => hdrOnly = !hdrOnly} title="Only HDR and Dolby Vision releases">
                        <i class="ri-contrast-2-line"></i> HDR<span class="seg-count">{hdrCount}</span>
                    </button>
                {/if}
                {#if subsCount > 0}
                    <button class="toggle-chip" class:active={subsOnly} on:click={() => subsOnly = !subsOnly} title="Only releases known to have English or multi-language subtitles">
                        <i class="ri-closed-captioning-line"></i> Subtitles<span class="seg-count">{subsCount}</span>
                    </button>
                {/if}
                {#if lowQualityCount > 0}
                    <button class="toggle-chip" class:active={!hideLowQuality} on:click={() => hideLowQuality = !hideLowQuality} title="Cam, telesync and screener rips">
                        <i class="ri-camera-off-line"></i> {hideLowQuality ? 'Show' : 'Hide'} cam rips<span class="seg-count">{lowQualityCount}</span>
                    </button>
                {/if}

            </div>

            <div class="toolbar">
                <div class="filter-input-wrap">
                    <i class="ri-search-2-line"></i>
                    <input type="text" placeholder="Filter results" bind:value={searchFilter} class="filter-input" />
                    {#if searchFilter}
                        <button class="icon-btn small" on:click={() => searchFilter = ""} aria-label="Clear filter"><i class="ri-close-line"></i></button>
                    {/if}
                </div>
                <span class="result-count">
                    {filteredResults.length === results.length ? `${results.length} results` : `${filteredResults.length} of ${results.length} results`}
                </span>
                <!-- svelte-ignore a11y-click-events-have-key-events -->
                <!-- svelte-ignore a11y-no-static-element-interactions -->
                <div class="sort-dropdown" on:click|stopPropagation>
                    <button class="sort-trigger" class:open={sortOpen} on:click={() => sortOpen = !sortOpen} aria-haspopup="listbox" aria-expanded={sortOpen}>
                        <span class="sort-caption">Sort</span>
                        {sortLabel}
                        <i class="ri-arrow-down-s-line sort-chevron"></i>
                    </button>
                    {#if sortOpen}
                        <div class="sort-menu" role="listbox" transition:scale={{ start: 0.96, duration: 140, opacity: 0 }}>
                            {#each SORT_OPTIONS as option (option.id)}
                                <button class="sort-item" class:active={sortBy === option.id} role="option" aria-selected={sortBy === option.id} on:click={() => pickSort(option.id)}>
                                    <i class={option.icon}></i>
                                    <span>{option.label}</span>
                                    {#if sortBy === option.id}<i class="ri-check-line sort-check"></i>{/if}
                                </button>
                            {/each}
                        </div>
                    {/if}
                </div>
            </div>
        {/if}

        <div class="results-list">
            {#if results.length === 0 && loading}
                <div class="loading-state">
                    <p>Searching {activeTrackerNames.join(', ') || 'trackers'}...</p>
                    {#each Array(5) as _}
                        <div class="skeleton-row">
                            <div class="skeleton skeleton-badge"></div>
                            <div class="skeleton-lines">
                                <div class="skeleton skeleton-line" style="width: 35%"></div>
                                <div class="skeleton skeleton-line" style="width: 80%"></div>
                            </div>
                        </div>
                    {/each}
                </div>
            {:else if results.length === 0}
                <div class="empty-state">
                    <i class="ri-file-search-line"></i>
                    <p>No torrents found</p>
                    <span>Try a shorter or different search, or pick other sources above.</span>
                </div>
            {:else if filteredResults.length === 0}
                <div class="empty-state">
                    <i class="ri-filter-off-line"></i>
                    <p>Nothing matches these filters</p>
                    <button class="text-btn" on:click={resetFilters}>Reset filters</button>
                </div>
            {:else}
                {#each sections as section (section.id)}
                    {@const hidden = section.collapsible && collapsedMismatch}
                    {@const limit = expanded[section.id] || PAGE_SIZE}
                    <section class="result-section" class:dimmed={section.id === 'mismatch'}>
                        <!-- svelte-ignore a11y-click-events-have-key-events -->
                        <div class="section-header" class:clickable={section.collapsible} on:click={() => section.collapsible && (collapsedMismatch = !collapsedMismatch)}>
                            <h4>{section.title}</h4>
                            <span class="section-count">{section.items.length}</span>
                            {#if section.collapsible}
                                <i class="ri-arrow-{hidden ? 'down' : 'up'}-s-line section-toggle"></i>
                            {/if}
                        </div>
                        {#if !hidden}
                            {#each section.items.slice(0, limit) as r (resultKey(r))}
                                {@const subs = r.subtitles ? SUB_LABELS[r.subtitles] : null}
                                {@const scope = scopeLabel(r)}
                                <button class="result {resolutionClass(r.resolution)}" class:best={resultKey(r) === bestResultKey} class:dead={r.seeds === 0} on:click={() => selectTorrent(r)} disabled={isSelectingTorrent}>
                                    <div class="res-badge">
                                        <span class="res-label">{r.resolution || '?'}</span>
                                        {#if r.source}<span class="res-source">{r.source}</span>{/if}
                                    </div>

                                    <div class="result-main">
                                        <div class="result-headline">
                                            {#if resultKey(r) === bestResultKey}
                                                <span class="pill pill-best"><i class="ri-star-fill"></i> Best match</span>
                                            {/if}
                                            {#if scope}<span class="pill pill-scope" class:pack={r.is_batch}>{scope}</span>{/if}
                                            {#each r.hdr || [] as hdr}
                                                <span class="pill pill-hdr" class:dv={hdr === 'DV'}>{hdr === 'DV' ? 'Dolby Vision' : hdr}</span>
                                            {/each}
                                            {#if subs}
                                                <span class="pill pill-subs tone-{subs.tone}" title={subs.title || ''}><i class={subs.icon}></i> {subs.label}</span>
                                            {/if}
                                            {#if languageLabel(r)}
                                                <span class="pill" title={r.languages?.join(', ')}><i class="ri-volume-up-line"></i> {languageLabel(r)}</span>
                                            {/if}
                                            {#each r.editions || [] as edition}
                                                <span class="pill">{edition}</span>
                                            {/each}
                                            {#if r.low_quality}
                                                <span class="pill tone-bad" title="Recorded in a cinema or from a screener, poor picture and sound"><i class="ri-error-warning-line"></i> Cam rip</span>
                                            {/if}
                                            {#if r.upscaled}
                                                <span class="pill tone-warn" title="Upscaled from a lower resolution">Upscaled</span>
                                            {/if}
                                        </div>

                                        <div class="result-title" title={r.title}>{r.title}</div>

                                        <div class="result-specs">
                                            {#if videoLabel(r)}<span><i class="ri-film-line"></i>{videoLabel(r)}</span>{/if}
                                            {#if audioLabel(r)}<span><i class="ri-music-2-line"></i>{audioLabel(r)}</span>{/if}
                                            {#if r.service}<span><i class="ri-tv-2-line"></i>{r.service}</span>{/if}
                                            {#if r.group}<span title="Release group"><i class="ri-group-line"></i>{r.group}</span>{/if}
                                            {#if r.file_count > 1}<span><i class="ri-file-copy-line"></i>{r.file_count} files</span>{/if}
                                            {#if r.uploaded}<span><i class="ri-time-line"></i>{age(r.uploaded)}</span>{/if}
                                            <span class="providers" title="Found on"><i class="ri-global-line"></i>{(r.providers?.length ? r.providers : [r.provider]).join(', ')}</span>
                                        </div>
                                    </div>

                                    <div class="result-stats" title="{r.seeds} seeders, {r.peers} leechers">
                                        <span class="stat-seeds health-{health(r.seeds)}">{r.seeds}<span class="stat-unit">seeds</span></span>
                                        <span class="stat-size">{r.size}</span>
                                    </div>
                                </button>
                            {/each}
                            {#if section.items.length > limit}
                                <button class="show-more" on:click={() => showMore(section.id)}>
                                    Show {Math.min(section.items.length - limit, PAGE_SIZE * 2)} more
                                </button>
                            {/if}
                        {/if}
                    </section>
                {/each}
            {/if}
        </div>

        <div class="custom-torrent-section" class:expanded={customTorrentExpanded}>
            {#if customTorrentExpanded}
                <div class="custom-torrent-inputs" transition:slide={{ duration: 280, easing: cubicOut }}>
                    <div class="custom-torrent-row">
                        <div class="magnet-input-wrapper" class:error={magnetError}>
                            <i class="ri-magnet-line"></i>
                            <input
                                type="text"
                                placeholder="Paste a magnet link"
                                bind:value={customMagnetLink}
                                on:input={handleMagnetInput}
                                class="magnet-input"
                            />
                            <button class="submit-magnet-btn" on:click={submitCustomMagnet} disabled={!customMagnetLink || magnetError} aria-label="Use magnet link">
                                <i class="ri-arrow-right-line"></i>
                            </button>
                        </div>
                        <button class="pick-file-btn" on:click={pickTorrentFile}>
                            <i class="ri-file-add-line"></i>
                            Pick .torrent file
                        </button>
                    </div>
                    {#if magnetError}
                        <span class="magnet-error">{magnetError}</span>
                    {/if}
                </div>
            {/if}
            <button class="custom-torrent-header" on:click={() => customTorrentExpanded = !customTorrentExpanded} aria-expanded={customTorrentExpanded}>
                <i class="ri-add-line toggle-icon"></i>
                <span>Use your own magnet link or .torrent file</span>
            </button>
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
