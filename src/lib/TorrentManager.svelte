<script>
  import { onMount, createEventDispatcher } from "svelte";
  import { fade, scale } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { invoke } from "@tauri-apps/api/core";

  export let media = null;
  export let details = null;
  export let allSeasonsData = {};
  export let refreshTrigger = 0;

  const dispatch = createEventDispatcher();

  let torrentData = {};
  let loading = true;
  let activeTab = "coverage";
  let expandedSeasons = new Set();

  $: isMovie = !details?.seasons || details.seasons.length === 0;

  // Per-season coverage: { [seasonNum]: { total, assignedCount, missing: [...episodes] } }
  $: seasonCoverage = (() => {
    const result = {};
    if (!details?.seasons) return result;
    for (const season of details.seasons.filter((s) => s.season_number > 0)) {
      const sNum = season.season_number;
      const episodes = allSeasonsData[sNum]?.episodes || [];
      const assigned = episodes.filter((ep) => torrentData[`${sNum}-${ep.episode_number}`]);
      const missing = episodes.filter((ep) => !torrentData[`${sNum}-${ep.episode_number}`]);
      result[sNum] = { total: episodes.length, assignedCount: assigned.length, missing };
    }
    return result;
  })();

  // Unique torrents with episode list for the Torrents tab
  $: uniqueTorrents = (() => {
    const map = new Map();
    for (const [key, info] of Object.entries(torrentData)) {
      const mag = info.magnetLink;
      if (!map.has(mag)) {
        map.set(mag, { magnetLink: mag, fileName: info.fileName, episodes: [] });
      }
      const [seasonStr, episodeStr] = key.split("-");
      map.get(mag).episodes.push({
        season: parseInt(seasonStr),
        episode: parseInt(episodeStr),
      });
    }
    return Array.from(map.values()).map((t) => ({
      ...t,
      episodes: t.episodes.sort((a, b) => a.season - b.season || a.episode - b.episode),
    }));
  })();

  $: if (refreshTrigger) {
    loadTorrentData();
  }

  onMount(() => {
    loadTorrentData();
  });

  async function loadTorrentData() {
    loading = true;
    torrentData = {};
    try {
      const allSelections = await invoke("get_all_torrent_selections", {
        showId: media.id,
      });
      if (allSelections) {
        if (isMovie) {
          const ep = allSelections.seasons?.["0"]?.episodes?.["0"];
          if (ep) {
            torrentData["0-0"] = {
              magnetLink: ep.magnet_link,
              fileIndex: ep.file_index,
              fileName: extractTorrentName(ep.magnet_link),
            };
          }
        } else {
          for (const [seasonNum, seasonData] of Object.entries(allSelections.seasons || {})) {
            for (const [episodeNum, epData] of Object.entries(seasonData.episodes || {})) {
              torrentData[`${seasonNum}-${episodeNum}`] = {
                magnetLink: epData.magnet_link,
                fileIndex: epData.file_index,
                fileName: extractTorrentName(epData.magnet_link),
              };
            }
          }
        }
      }
    } catch (err) {
      console.error("error loading torrent data:", err);
    }
    loading = false;
  }

  function extractTorrentName(magnetLink) {
    if (!magnetLink) return "Unknown";
    const m = magnetLink.match(/dn=([^&]+)/);
    return m ? decodeURIComponent(m[1].replace(/\+/g, " ")) : "Unknown";
  }

  function toggleSeason(seasonNum) {
    if (expandedSeasons.has(seasonNum)) {
      expandedSeasons.delete(seasonNum);
    } else {
      expandedSeasons.add(seasonNum);
    }
    expandedSeasons = expandedSeasons;
  }

  function handleAssign(season, episode) {
    dispatch("selectTorrent", { season, episode });
  }

  async function handleRemoveTorrentByMagnet(magnetLink) {
    try {
      await invoke("remove_torrent_all_assignments", {
        showId: media.id,
        magnetLink,
      });
      await loadTorrentData();
    } catch (err) {
      console.error("error removing torrent assignments:", err);
    }
  }

  function formatCoverage(episodes) {
    const bySeason = {};
    for (const ep of episodes) {
      if (!bySeason[ep.season]) bySeason[ep.season] = [];
      bySeason[ep.season].push(ep.episode);
    }
    const parts = [];
    for (const [s, eps] of Object.entries(bySeason).sort(([a], [b]) => Number(a) - Number(b))) {
      const sorted = eps.slice().sort((a, b) => a - b);
      const ranges = [];
      let start = sorted[0];
      let end = sorted[0];
      for (let i = 1; i < sorted.length; i++) {
        if (sorted[i] === end + 1) {
          end = sorted[i];
        } else {
          ranges.push(start === end ? `E${start}` : `E${start}–${end}`);
          start = end = sorted[i];
        }
      }
      ranges.push(start === end ? `E${start}` : `E${start}–${end}`);
      parts.push(`S${s}: ${ranges.join(", ")}`);
    }
    return parts.join(" · ");
  }

  function close() {
    dispatch("close");
  }
</script>

<div class="torrent-manager-overlay" transition:fade|global={{ duration: 400, easing: cubicOut }} on:click={close}>
  <div class="torrent-manager-modal" in:scale|global={{ start: 1.08, opacity: 0, duration: 400, easing: cubicOut }} out:scale|global={{ start: 1.08, opacity: 1, duration: 400, easing: cubicOut }} on:click|stopPropagation>
    <div class="torrent-manager-header">
      <div class="header-left">
        <h2>Torrent Manager</h2>
        <span class="header-subtitle">{details?.title || details?.name || ""}</span>
      </div>
    </div>

    {#if !isMovie}
      <div class="tab-bar">
        <button
          class="tab-btn"
          class:active={activeTab === "coverage"}
          on:click={() => (activeTab = "coverage")}
        >
          <i class="ri-layout-grid-line"></i>
          Coverage
        </button>
        <button
          class="tab-btn"
          class:active={activeTab === "torrents"}
          on:click={() => (activeTab = "torrents")}
        >
          <i class="ri-magnet-line"></i>
          Assigned Torrents
          {#if uniqueTorrents.length > 0}
            <span class="tab-badge">{uniqueTorrents.length}</span>
          {/if}
        </button>
      </div>
    {/if}

    <div class="torrent-manager-content">
      {#if loading}
        <div class="loading-state">
          <div class="loading-spinner"></div>
          <p>Loading torrent assignments…</p>
        </div>
      {:else if isMovie}
        <div class="movie-section">
          {#if torrentData["0-0"]}
            <div class="movie-assigned">
              <div class="assigned-badge">
                <i class="ri-check-line"></i>
                Torrent Assigned
              </div>
              <p class="movie-torrent-name">{torrentData["0-0"].fileName}</p>
              <button class="btn-change-movie" on:click={() => handleAssign(0, 0)}>
                <i class="ri-refresh-line"></i>
                Change Torrent
              </button>
            </div>
          {:else}
            <div class="movie-unassigned">
              <i class="ri-film-line movie-icon"></i>
              <p>No torrent assigned for this movie.</p>
              <button class="btn-assign-primary" on:click={() => handleAssign(0, 0)}>
                <i class="ri-search-line"></i>
                Select Torrent
              </button>
            </div>
          {/if}
        </div>
      {:else if activeTab === "coverage"}
        <div class="seasons-list">
          {#each (details?.seasons?.filter((s) => s.season_number > 0) || []) as season}
            {@const sNum = season.season_number}
            {@const cov = seasonCoverage[sNum] || { total: 0, assignedCount: 0, missing: [] }}
            {@const isExpanded = expandedSeasons.has(sNum)}
            {@const isFullyCovered = cov.total > 0 && cov.assignedCount === cov.total}
            {@const hasNone = cov.assignedCount === 0}
            <div class="season-row" class:fully-covered={isFullyCovered}>
              <button class="season-toggle" on:click={() => toggleSeason(sNum)}>
                <div class="season-toggle-left">
                  <span class="season-label">Season {sNum}</span>
                  {#if cov.total === 0}
                    <span class="coverage-badge loading-badge">Loading…</span>
                  {:else if isFullyCovered}
                    <span class="coverage-badge full">
                      <i class="ri-check-double-line"></i>
                      {cov.assignedCount}/{cov.total}
                    </span>
                  {:else if hasNone}
                    <span class="coverage-badge none">{cov.total} episodes unassigned</span>
                  {:else}
                    <span class="coverage-badge partial">
                      {cov.assignedCount}/{cov.total} · {cov.missing.length} missing
                    </span>
                  {/if}
                </div>
                <div class="season-toggle-right">
                  {#if !isFullyCovered && cov.total > 0}
                    <button
                      class="btn-assign-season"
                      on:click|stopPropagation={() =>
                        handleAssign(sNum, cov.missing[0]?.episode_number ?? 1)}
                      title="Assign {hasNone ? 'this season' : 'missing episodes'}"
                    >
                      <i class="ri-add-line"></i>
                      {hasNone ? "Assign Season" : "Assign Missing"}
                    </button>
                  {/if}
                  <i class="ri-arrow-down-s-line chevron" class:rotated={isExpanded}></i>
                </div>
              </button>

              {#if isExpanded}
                <div class="episodes-list">
                  {#each allSeasonsData[sNum]?.episodes || [] as ep}
                    {@const key = `${sNum}-${ep.episode_number}`}
                    {@const torrent = torrentData[key]}
                    <div class="episode-row" class:ep-assigned={!!torrent}>
                      <div class="ep-number">E{ep.episode_number}</div>
                      <div class="ep-name">{ep.name}</div>
                      {#if torrent}
                        <div class="ep-torrent-chip" title={torrent.fileName}>
                          <i class="ri-check-line"></i>
                          <span>{torrent.fileName}</span>
                        </div>
                      {:else}
                        <button
                          class="btn-assign-ep"
                          on:click={() => handleAssign(sNum, ep.episode_number)}
                        >
                          <i class="ri-add-line"></i>
                          Assign
                        </button>
                      {/if}
                    </div>
                  {/each}
                  {#if !allSeasonsData[sNum]?.episodes?.length}
                    <div class="ep-loading">Loading episodes…</div>
                  {/if}
                </div>
              {/if}
            </div>
          {/each}
        </div>
      {:else if activeTab === "torrents"}
        {#if uniqueTorrents.length === 0}
          <div class="empty-torrents">
            <i class="ri-magnet-line"></i>
            <p>No torrents assigned yet.</p>
            <p class="empty-sub">Switch to Coverage to assign torrents to episodes.</p>
          </div>
        {:else}
          <div class="torrents-list">
            {#each uniqueTorrents as torrent}
              <div class="torrent-card">
                <div class="torrent-card-info">
                  <div class="torrent-card-name" title={torrent.fileName}>
                    <i class="ri-file-line"></i>
                    <span>{torrent.fileName}</span>
                  </div>
                  <div class="torrent-coverage-summary">
                    {formatCoverage(torrent.episodes)}
                  </div>
                  <div class="torrent-ep-count">
                    {torrent.episodes.length}
                    {torrent.episodes.length === 1 ? "episode" : "episodes"} assigned
                  </div>
                </div>
                <button
                  class="btn-remove-torrent"
                  on:click={() => handleRemoveTorrentByMagnet(torrent.magnetLink)}
                  title="Remove all assignments for this torrent"
                >
                  <i class="ri-delete-bin-line"></i>
                  Remove
                </button>
              </div>
            {/each}
          </div>
        {/if}
      {/if}
    </div>
  </div>
</div>

<style>
  .torrent-manager-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.85);
    backdrop-filter: blur(12px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 10000;
  }

  .torrent-manager-modal {
    background: rgba(14, 14, 14, 0.98);
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: var(--border-radius-lg);
    width: 92%;
    max-width: 820px;
    max-height: 86vh;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    box-shadow: 0 24px 64px rgba(0, 0, 0, 0.7);
  }

  .torrent-manager-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 20px 24px 16px;
    border-bottom: 1px solid rgba(255, 255, 255, 0.07);
    flex-shrink: 0;
  }

  .header-left {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .torrent-manager-header h2 {
    font-size: 18px;
    font-weight: 600;
    margin: 0;
    color: var(--text-primary);
    letter-spacing: -0.01em;
  }

  .header-subtitle {
    font-size: 12px;
    color: var(--text-tertiary);
  }

  .tab-bar {
    display: flex;
    gap: 4px;
    padding: 12px 24px;
    border-bottom: 1px solid rgba(255, 255, 255, 0.06);
    background: rgba(255, 255, 255, 0.02);
    flex-shrink: 0;
  }

  .tab-btn {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 7px 14px;
    background: transparent;
    border: 1px solid transparent;
    color: var(--text-secondary);
    font-size: 13px;
    font-weight: 500;
    border-radius: var(--border-radius-sm);
    cursor: pointer;
    transition: all 0.15s ease;
    font-family: inherit;
  }

  .tab-btn i {
    font-size: 15px;
  }

  .tab-btn:hover {
    background: rgba(255, 255, 255, 0.05);
    color: var(--text-primary);
  }

  .tab-btn.active {
    background: rgba(255, 255, 255, 0.08);
    border-color: rgba(255, 255, 255, 0.12);
    color: var(--text-primary);
  }

  .tab-badge {
    background: var(--accent-color);
    color: #000;
    font-size: 10px;
    font-weight: 700;
    padding: 1px 6px;
    border-radius: 10px;
    min-width: 18px;
    text-align: center;
  }

  .torrent-manager-content {
    flex: 1;
    overflow-y: auto;
    padding: 16px 24px 24px;
  }

  .loading-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 60px 24px;
    gap: 14px;
  }

  .loading-spinner {
    width: 36px;
    height: 36px;
    border: 2px solid rgba(255, 255, 255, 0.08);
    border-top-color: var(--accent-color);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  .loading-state p {
    color: var(--text-secondary);
    font-size: 14px;
    margin: 0;
  }

  .movie-section {
    display: flex;
    justify-content: center;
    padding: 24px 0;
  }

  .movie-assigned {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 10px;
    background: rgba(46, 213, 115, 0.05);
    border: 1px solid rgba(46, 213, 115, 0.2);
    border-radius: var(--border-radius-md);
    padding: 20px 24px;
    width: 100%;
  }

  .movie-unassigned {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 14px;
    padding: 40px 24px;
    color: var(--text-secondary);
    text-align: center;
  }

  .movie-icon {
    font-size: 40px;
    opacity: 0.3;
  }

  .movie-unassigned p {
    margin: 0;
    font-size: 14px;
  }

  .assigned-badge {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: #2ed573;
  }

  .assigned-badge i {
    font-size: 14px;
  }

  .movie-torrent-name {
    font-size: 14px;
    color: var(--text-primary);
    margin: 0;
    word-break: break-all;
  }

  .btn-change-movie {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 7px 14px;
    background: rgba(255, 255, 255, 0.06);
    border: 1px solid rgba(255, 255, 255, 0.1);
    color: var(--text-secondary);
    font-size: 13px;
    font-weight: 500;
    border-radius: var(--border-radius-sm);
    cursor: pointer;
    transition: all 0.15s ease;
    font-family: inherit;
    margin-top: 4px;
  }

  .btn-change-movie:hover {
    background: rgba(255, 255, 255, 0.1);
    border-color: rgba(255, 255, 255, 0.18);
    color: var(--text-primary);
  }

  .btn-assign-primary {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 20px;
    background: var(--accent-color);
    border: none;
    color: #000;
    font-size: 14px;
    font-weight: 600;
    border-radius: var(--border-radius-md);
    cursor: pointer;
    transition: all 0.15s ease;
    font-family: inherit;
  }

  .btn-assign-primary:hover {
    opacity: 0.9;
    transform: translateY(-1px);
  }

  .seasons-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .season-row {
    background: rgba(255, 255, 255, 0.03);
    border: 1px solid rgba(255, 255, 255, 0.07);
    border-radius: var(--border-radius-md);
    overflow: hidden;
    transition: border-color 0.15s ease;
  }

  .season-row.fully-covered {
    border-color: rgba(46, 213, 115, 0.2);
    background: rgba(46, 213, 115, 0.03);
  }

  .season-toggle {
    width: 100%;
    background: transparent;
    border: none;
    padding: 14px 18px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    cursor: pointer;
    transition: background 0.15s ease;
    text-align: left;
  }

  .season-toggle:hover {
    background: rgba(255, 255, 255, 0.04);
  }

  .season-toggle-left {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .season-toggle-right {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .season-label {
    font-size: 15px;
    font-weight: 600;
    color: var(--text-primary);
    white-space: nowrap;
  }

  .coverage-badge {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 12px;
    font-weight: 500;
    padding: 3px 10px;
    border-radius: 20px;
  }

  .coverage-badge.full {
    background: rgba(46, 213, 115, 0.12);
    color: #2ed573;
    border: 1px solid rgba(46, 213, 115, 0.25);
  }

  .coverage-badge.partial {
    background: rgba(251, 191, 36, 0.1);
    color: #fbbf24;
    border: 1px solid rgba(251, 191, 36, 0.25);
  }

  .coverage-badge.none {
    background: rgba(255, 255, 255, 0.05);
    color: var(--text-tertiary);
    border: 1px solid rgba(255, 255, 255, 0.08);
  }

  .coverage-badge.loading-badge {
    background: rgba(255, 255, 255, 0.05);
    color: var(--text-tertiary);
    border: 1px solid rgba(255, 255, 255, 0.08);
  }

  .coverage-badge i {
    font-size: 13px;
  }

  .btn-assign-season {
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 6px 12px;
    background: rgba(211, 118, 195, 0.12);
    border: 1px solid rgba(211, 118, 195, 0.3);
    color: var(--accent-color);
    font-size: 12px;
    font-weight: 600;
    border-radius: var(--border-radius-sm);
    cursor: pointer;
    transition: all 0.15s ease;
    white-space: nowrap;
    font-family: inherit;
  }

  .btn-assign-season:hover {
    background: rgba(211, 118, 195, 0.2);
    border-color: rgba(211, 118, 195, 0.5);
  }

  .chevron {
    font-size: 18px;
    color: var(--text-tertiary);
    transition: transform 0.2s ease;
    flex-shrink: 0;
  }

  .chevron.rotated {
    transform: rotate(180deg);
  }

  .episodes-list {
    border-top: 1px solid rgba(255, 255, 255, 0.05);
    display: flex;
    flex-direction: column;
  }

  .episode-row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 18px;
    border-bottom: 1px solid rgba(255, 255, 255, 0.03);
    transition: background 0.1s ease;
  }

  .episode-row:last-child {
    border-bottom: none;
  }

  .episode-row:hover {
    background: rgba(255, 255, 255, 0.02);
  }

  .ep-number {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-tertiary);
    font-family: "Geist Mono Variable", monospace;
    width: 36px;
    flex-shrink: 0;
  }

  .ep-name {
    flex: 1;
    font-size: 13px;
    color: var(--text-secondary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }

  .episode-row.ep-assigned .ep-name {
    color: var(--text-primary);
  }

  .ep-torrent-chip {
    display: flex;
    align-items: center;
    gap: 5px;
    background: rgba(46, 213, 115, 0.08);
    border: 1px solid rgba(46, 213, 115, 0.2);
    color: #2ed573;
    font-size: 11px;
    padding: 3px 10px;
    border-radius: 20px;
    max-width: 220px;
    flex-shrink: 0;
    overflow: hidden;
  }

  .ep-torrent-chip i {
    font-size: 12px;
    flex-shrink: 0;
  }

  .ep-torrent-chip span {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .btn-assign-ep {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 5px 12px;
    background: rgba(255, 255, 255, 0.05);
    border: 1px solid rgba(255, 255, 255, 0.1);
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 500;
    border-radius: var(--border-radius-sm);
    cursor: pointer;
    transition: all 0.15s ease;
    white-space: nowrap;
    flex-shrink: 0;
    font-family: inherit;
  }

  .btn-assign-ep:hover {
    background: rgba(255, 255, 255, 0.1);
    border-color: rgba(255, 255, 255, 0.18);
    color: var(--text-primary);
  }

  .ep-loading {
    padding: 14px 18px;
    font-size: 13px;
    color: var(--text-tertiary);
  }

  .empty-torrents {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 60px 24px;
    gap: 10px;
    color: var(--text-secondary);
    text-align: center;
  }

  .empty-torrents i {
    font-size: 40px;
    opacity: 0.25;
  }

  .empty-torrents p {
    margin: 0;
    font-size: 14px;
  }

  .empty-sub {
    font-size: 12px !important;
    color: var(--text-tertiary) !important;
  }

  .torrents-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .torrent-card {
    display: flex;
    align-items: flex-start;
    gap: 16px;
    padding: 16px 18px;
    background: rgba(255, 255, 255, 0.03);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: var(--border-radius-md);
    transition: border-color 0.15s ease;
  }

  .torrent-card:hover {
    border-color: rgba(255, 255, 255, 0.12);
  }

  .torrent-card-info {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
  }

  .torrent-card-name {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    color: var(--text-primary);
    font-size: 14px;
    font-weight: 500;
    line-height: 1.4;
  }

  .torrent-card-name i {
    color: var(--text-tertiary);
    font-size: 16px;
    margin-top: 1px;
    flex-shrink: 0;
  }

  .torrent-card-name span {
    word-break: break-all;
  }

  .torrent-coverage-summary {
    font-size: 12px;
    color: var(--text-secondary);
    font-family: "Geist Mono Variable", monospace;
    padding-left: 24px;
  }

  .torrent-ep-count {
    font-size: 11px;
    color: var(--text-tertiary);
    padding-left: 24px;
  }

  .btn-remove-torrent {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 7px 14px;
    background: rgba(239, 68, 68, 0.08);
    border: 1px solid rgba(239, 68, 68, 0.2);
    color: #f87171;
    font-size: 12px;
    font-weight: 500;
    border-radius: var(--border-radius-sm);
    cursor: pointer;
    transition: all 0.15s ease;
    white-space: nowrap;
    flex-shrink: 0;
    font-family: inherit;
    margin-top: 2px;
  }

  .btn-remove-torrent:hover {
    background: rgba(239, 68, 68, 0.15);
    border-color: rgba(239, 68, 68, 0.35);
    color: #fca5a5;
  }

  @keyframes fadeIn {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  @keyframes slideUp {
    from { transform: translateY(16px); opacity: 0; }
    to { transform: translateY(0); opacity: 1; }
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }
</style>
