<script>
  import { onMount, createEventDispatcher } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getImageUrl } from "./tmdb.js";

  export let media = null;
  export let details = null;
  export let allSeasonsData = {};
  export let refreshTrigger = 0;

  const dispatch = createEventDispatcher();

  let torrentData = {};
  let loading = true;
  let selectedSeason = null;
  let expandedSeasons = new Set();

  $: isMovie = !details?.seasons || details.seasons.length === 0;

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
          if (allSelections.seasons && allSelections.seasons["0"] && allSelections.seasons["0"].episodes && allSelections.seasons["0"].episodes["0"]) {
            const saved = allSelections.seasons["0"].episodes["0"];
            torrentData["0-0"] = {
              magnetLink: saved.magnet_link,
              fileIndex: saved.file_index,
              fileName: extractTorrentNameFromMagnet(saved.magnet_link),
            };
          }
        } else {
          for (const [seasonNum, seasonData] of Object.entries(allSelections.seasons || {})) {
            for (const [episodeNum, episodeData] of Object.entries(seasonData.episodes || {})) {
              torrentData[`${seasonNum}-${episodeNum}`] = {
                magnetLink: episodeData.magnet_link,
                fileIndex: episodeData.file_index,
                fileName: extractTorrentNameFromMagnet(episodeData.magnet_link),
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

  function extractTorrentNameFromMagnet(magnetLink) {
    if (!magnetLink) return "Unknown";
    const dnMatch = magnetLink.match(/dn=([^&]+)/);
    if (dnMatch) {
      return decodeURIComponent(dnMatch[1].replace(/\+/g, " "));
    }
    return "Unknown";
  }

  function toggleSeason(seasonNum) {
    if (expandedSeasons.has(seasonNum)) {
      expandedSeasons.delete(seasonNum);
    } else {
      expandedSeasons.add(seasonNum);
    }
    expandedSeasons = expandedSeasons;
  }

  function handleSelectTorrent(season, episode) {
    dispatch("selectTorrent", { season, episode });
  }

  function handleRemoveTorrent(season, episode) {
    dispatch("removeTorrent", { season, episode });
  }

  function close() {
    dispatch("close");
  }

  function getEpisodeName(seasonNum, episodeNum) {
    const seasonEpisodes = allSeasonsData[seasonNum]?.episodes || [];
    const episode = seasonEpisodes.find((e) => e.episode_number === episodeNum);
    return episode?.name || `Episode ${episodeNum}`;
  }
</script>

<div class="torrent-manager-overlay">
  <div class="torrent-manager-modal">
    <div class="torrent-manager-header">
      <h2>Torrent Manager</h2>
      <button class="btn-close" on:click={close}>
        <i class="ri-close-line"></i>
      </button>
    </div>

    <div class="torrent-manager-content">
      {#if loading}
        <div class="loading-state">
          <div class="loading-spinner"></div>
          <p>Loading torrent assignments...</p>
        </div>
      {:else if isMovie}
        <div class="movie-section">
          <div class="movie-header">
            <h3>{details.title || details.name}</h3>
          </div>
          {#if torrentData["0-0"]}
            <div class="torrent-item assigned">
              <div class="torrent-info">
                <div class="torrent-status">
                  <i class="ri-check-line"></i>
                  <span>Assigned</span>
                </div>
                <div class="torrent-details">
                  <p class="torrent-name">{torrentData["0-0"].fileName}</p>
                  <p class="file-info">File Index: {torrentData["0-0"].fileIndex}</p>
                </div>
              </div>
              <div class="torrent-actions">
                <button
                  class="btn-standard secondary"
                  on:click={() => handleSelectTorrent(0, 0)}
                  title="Change torrent"
                >
                  <i class="ri-refresh-line"></i>
                </button>
              </div>
            </div>
          {:else}
            <div class="torrent-item unassigned">
              <div class="torrent-info">
                <div class="torrent-status">
                  <i class="ri-close-line"></i>
                  <span>No Torrent Assigned</span>
                </div>
              </div>
              <div class="torrent-actions">
                <button
                  class="btn-standard primary"
                  on:click={() => handleSelectTorrent(0, 0)}
                >
                  <i class="ri-add-line"></i>
                  Select Torrent
                </button>
              </div>
            </div>
          {/if}
        </div>
      {:else}
        <div class="series-section">
          {#each details.seasons.filter((s) => s.season_number > 0) as season}
            {@const seasonNum = season.season_number}
            {@const seasonEpisodes = allSeasonsData[seasonNum]?.episodes || []}
            {@const assignedCount = seasonEpisodes.filter((ep) => torrentData[`${seasonNum}-${ep.episode_number}`]).length}
            <div class="season-group">
              <button
                class="season-header"
                class:expanded={expandedSeasons.has(seasonNum)}
                on:click={() => toggleSeason(seasonNum)}
              >
                <div class="season-title">
                  <span class="season-name">Season {seasonNum}</span>
                  <span class="assignment-count">
                    {assignedCount}/{seasonEpisodes.length} assigned
                  </span>
                </div>
                <i class="ri-arrow-down-s-line"></i>
              </button>

              {#if expandedSeasons.has(seasonNum)}
                <div class="episodes-grid">
                  {#each seasonEpisodes as episode}
                    {@const key = `${seasonNum}-${episode.episode_number}`}
                    {@const torrent = torrentData[key]}
                    <div class="episode-card" class:assigned={!!torrent}>
                      {#if episode.still_path}
                        <div class="episode-background">
                          <img src={getImageUrl(episode.still_path, "w300")} alt="" />
                          <div class="episode-gradient"></div>
                        </div>
                      {/if}
                      <div class="episode-content">
                        <div class="episode-header">
                          <span class="episode-num">E{episode.episode_number}</span>
                          {#if torrent}
                            <i class="ri-check-line status-icon assigned"></i>
                          {:else}
                            <i class="ri-close-line status-icon unassigned"></i>
                          {/if}
                        </div>
                        <div class="episode-name">{episode.name}</div>
                        {#if torrent}
                          <div class="torrent-filename" title={torrent.fileName}>{torrent.fileName}</div>
                        {/if}
                        <button
                          class="episode-action-btn"
                          on:click={() => handleSelectTorrent(seasonNum, episode.episode_number)}
                          title={torrent ? "Change torrent" : "Select torrent"}
                        >
                          <i class="{torrent ? 'ri-refresh-line' : 'ri-add-line'}"></i>
                        </button>
                      </div>
                    </div>
                  {/each}
                </div>
              {/if}
            </div>
          {/each}
        </div>
      {/if}
    </div>
  </div>
</div>

<style>
  .torrent-manager-overlay {
    position: fixed;
    top: 0;
    left: 0;
    width: 100%;
    height: 100%;
    background: rgba(0, 0, 0, 0.85);
    backdrop-filter: blur(12px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 10000;
    animation: fadeIn 0.25s cubic-bezier(0.4, 0, 0.2, 1);
  }

  .torrent-manager-modal {
    background: rgba(15, 15, 15, 0.98);
    backdrop-filter: blur(20px);
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: var(--border-radius-lg);
    width: 90%;
    max-width: 900px;
    max-height: 85vh;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    box-shadow: 0 20px 60px rgba(0, 0, 0, 0.6);
    animation: slideUp 0.3s cubic-bezier(0.4, 0, 0.2, 1);
  }

  .torrent-manager-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: var(--spacing-2xl) var(--spacing-2xl) var(--spacing-lg);
    border-bottom: 1px solid rgba(255, 255, 255, 0.08);
  }

  .torrent-manager-header h2 {
    font-size: 20px;
    font-weight: 600;
    margin: 0;
    color: var(--text-primary);
  }

  .btn-close {
    background: rgba(255, 255, 255, 0.05);
    border: 1px solid rgba(255, 255, 255, 0.1);
    color: var(--text-secondary);
    font-size: 20px;
    cursor: pointer;
    padding: 8px;
    width: 36px;
    height: 36px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--border-radius-md);
    transition: all 0.2s cubic-bezier(0.4, 0, 0.2, 1);
  }

  .btn-close:hover {
    background: rgba(255, 255, 255, 0.1);
    border-color: rgba(255, 255, 255, 0.2);
    color: var(--text-primary);
    transform: scale(1.05);
  }

  .torrent-manager-content {
    padding: 24px;
    overflow-y: auto;
    flex: 1;
  }

  .loading-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 60px 24px;
    gap: 16px;
  }

  .loading-spinner {
    width: 40px;
    height: 40px;
    border: 3px solid var(--border-color);
    border-top-color: var(--accent-color);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  .loading-state p {
    color: var(--text-secondary);
    font-size: 14px;
  }

  .movie-section {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .movie-header h3 {
    font-size: 20px;
    font-weight: 600;
    margin: 0 0 16px 0;
    color: var(--text-primary);
  }

  .torrent-item {
    background: rgba(255, 255, 255, 0.03);
    backdrop-filter: blur(10px);
    border-radius: var(--border-radius-md);
    padding: var(--spacing-lg) var(--spacing-xl);
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: var(--spacing-lg);
    transition: all 0.2s cubic-bezier(0.4, 0, 0.2, 1);
  }

  .torrent-item.assigned {
    border: 1px solid rgba(46, 213, 115, 0.25);
    background: rgba(46, 213, 115, 0.04);
  }

  .torrent-item.unassigned {
    border: 1px solid rgba(255, 255, 255, 0.08);
  }

  .torrent-info {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .torrent-status {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    font-weight: 500;
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  .torrent-item.assigned .torrent-status {
    color: #2ed573;
  }

  .torrent-item.unassigned .torrent-status {
    color: var(--text-secondary);
  }

  .torrent-status i {
    font-size: 16px;
  }

  .torrent-details {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .torrent-name {
    font-size: 14px;
    color: var(--text-primary);
    margin: 0;
    word-break: break-word;
  }

  .file-info {
    font-size: 12px;
    color: var(--text-secondary);
    margin: 0;
  }

  .torrent-actions {
    display: flex;
    gap: 8px;
  }

  .series-section {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .season-group {
    background: rgba(255, 255, 255, 0.03);
    backdrop-filter: blur(10px);
    border-radius: var(--border-radius-md);
    overflow: hidden;
    border: 1px solid rgba(255, 255, 255, 0.08);
    transition: all 0.2s cubic-bezier(0.4, 0, 0.2, 1);
  }

  .season-group:hover {
    border-color: rgba(255, 255, 255, 0.12);
  }

  .season-header {
    width: 100%;
    background: transparent;
    border: none;
    padding: var(--spacing-lg) var(--spacing-xl);
    display: flex;
    justify-content: space-between;
    align-items: center;
    cursor: pointer;
    transition: all 0.2s cubic-bezier(0.4, 0, 0.2, 1);
  }

  .season-header:hover {
    background: rgba(255, 255, 255, 0.05);
  }

  .season-header i {
    font-size: 20px;
    color: var(--text-secondary);
    transition: transform 0.3s;
  }

  .season-header.expanded i {
    transform: rotate(180deg);
  }

  .season-title {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 4px;
  }

  .season-name {
    font-size: 16px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .assignment-count {
    font-size: 12px;
    color: var(--text-secondary);
  }

  .episodes-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
    gap: var(--spacing-md);
    padding: var(--spacing-lg);
  }

  .episode-card {
    position: relative;
    aspect-ratio: 16/9;
    border-radius: var(--border-radius-md);
    overflow: hidden;
    border: 1px solid rgba(255, 255, 255, 0.08);
    transition: all 0.2s cubic-bezier(0.4, 0, 0.2, 1);
    background: rgba(255, 255, 255, 0.02);
  }

  .episode-card:hover {
    border-color: rgba(255, 255, 255, 0.15);
    transform: translateY(-2px);
    box-shadow: 0 8px 16px rgba(0, 0, 0, 0.4);
  }

  .episode-card.assigned {
    border-color: rgba(46, 213, 115, 0.3);
  }

  .episode-background {
    position: absolute;
    inset: 0;
    z-index: 0;
  }

  .episode-background img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    opacity: 0.3;
  }

  .episode-gradient {
    position: absolute;
    inset: 0;
    background: linear-gradient(to bottom, rgba(0, 0, 0, 0.4) 0%, rgba(0, 0, 0, 0.8) 100%);
  }

  .episode-content {
    position: relative;
    z-index: 1;
    height: 100%;
    display: flex;
    flex-direction: column;
    padding: var(--spacing-sm);
  }

  .episode-card .episode-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: auto;
  }

  .episode-card .episode-num {
    font-size: 11px;
    font-weight: 700;
    color: var(--text-primary);
    background: rgba(0, 0, 0, 0.6);
    backdrop-filter: blur(8px);
    padding: 4px 8px;
    border-radius: var(--border-radius-sm);
  }

  .status-icon {
    font-size: 14px;
    background: rgba(0, 0, 0, 0.6);
    backdrop-filter: blur(8px);
    padding: 4px;
    border-radius: 50%;
    width: 22px;
    height: 22px;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .status-icon.assigned {
    color: #2ed573;
  }

  .status-icon.unassigned {
    color: var(--text-secondary);
  }

  .episode-name {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-primary);
    line-height: 1.3;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    text-shadow: 0 1px 4px rgba(0, 0, 0, 0.8);
    margin-bottom: var(--spacing-xs);
  }

  .torrent-filename {
    font-size: 10px;
    color: var(--text-secondary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    margin-bottom: var(--spacing-xs);
    opacity: 0.8;
    text-shadow: 0 1px 4px rgba(0, 0, 0, 0.8);
  }

  .episode-action-btn {
    width: 100%;
    padding: 6px;
    background: rgba(255, 255, 255, 0.15);
    backdrop-filter: blur(8px);
    border: 1px solid rgba(255, 255, 255, 0.2);
    border-radius: var(--border-radius-sm);
    color: var(--text-primary);
    cursor: pointer;
    transition: all 0.2s cubic-bezier(0.4, 0, 0.2, 1);
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 14px;
  }

  .episode-action-btn:hover {
    background: rgba(255, 255, 255, 0.25);
    border-color: rgba(255, 255, 255, 0.3);
    transform: scale(1.02);
  }

  @keyframes fadeIn {
    from {
      opacity: 0;
    }
    to {
      opacity: 1;
    }
  }

  @keyframes slideUp {
    from {
      transform: translateY(20px);
      opacity: 0;
    }
    to {
      transform: translateY(0);
      opacity: 1;
    }
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
