<script>
  import { onMount, createEventDispatcher } from "svelte";
  import { fade, scale } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { invoke } from "@tauri-apps/api/core";
  import { open as openFileDialog } from "@tauri-apps/plugin-dialog";

  export let media = null;
  export let details = null;
  export let allSeasonsData = {};
  export let refreshTrigger = 0;

  const dispatch = createEventDispatcher();

  let coverage = {};   // { "1": [1, 3, 5], "2": [1, 2] }
  let loading = true;
  let importing = false;
  let importResult = null;
  let expandedSeasons = new Set();
  let showImportMenu = false;

  $: seasonCount = details?.seasons?.filter(s => s.season_number > 0).length ?? 1;

  $: if (refreshTrigger !== undefined) {
    loadCoverage();
  }

  onMount(() => {
    loadCoverage();
  });

  async function loadCoverage() {
    loading = true;
    try {
      coverage = await invoke("get_subtitle_pack_coverage", { showId: media.id });
    } catch (err) {
      console.error("error loading subtitle coverage:", err);
    }
    loading = false;
  }

  function episodeHasSub(season, episodeNum) {
    return (coverage[String(season)] ?? []).includes(episodeNum);
  }

  function seasonSubCount(seasonNum) {
    return (coverage[String(seasonNum)] ?? []).length;
  }

  function toggleSeason(seasonNum) {
    if (expandedSeasons.has(seasonNum)) {
      expandedSeasons.delete(seasonNum);
    } else {
      expandedSeasons.add(seasonNum);
    }
    expandedSeasons = expandedSeasons;
  }

  async function importFiles() {
    showImportMenu = false;
    const selected = await openFileDialog({
      multiple: true,
      filters: [
        { name: "Subtitles & Archives", extensions: ["srt", "ass", "ssa", "vtt", "sub", "zip"] },
      ],
    });
    if (!selected) return;
    const paths = Array.isArray(selected)
      ? selected.map(f => (typeof f === "string" ? f : f.path))
      : [typeof selected === "string" ? selected : selected.path];
    await doImport(paths);
  }

  async function importFolder() {
    showImportMenu = false;
    const selected = await openFileDialog({
      directory: true,
      multiple: false,
    });
    if (!selected) return;
    const path = typeof selected === "string" ? selected : selected?.path;
    if (!path) return;
    await doImport([path]);
  }

  async function doImport(paths) {
    importing = true;
    importResult = null;
    try {
      const result = await invoke("import_subtitle_pack", {
        showId: media.id,
        paths,
      });
      importResult = result;
      await loadCoverage();
    } catch (err) {
      importResult = { imported: 0, skipped: 0, errors: [String(err)] };
    }
    importing = false;
  }

  async function removeEpisode(season, episode) {
    try {
      await invoke("remove_subtitle_pack_episode", {
        showId: media.id,
        season,
        episode,
      });
      await loadCoverage();
    } catch (err) {
      console.error("error removing subtitle:", err);
    }
  }

  async function clearAll() {
    try {
      await invoke("clear_subtitle_pack", { showId: media.id });
      coverage = {};
      importResult = null;
    } catch (err) {
      console.error("error clearing subtitle packs:", err);
    }
  }

  function close() {
    dispatch("close");
  }
</script>

<!-- svelte-ignore a11y-click-events-have-key-events -->
<!-- svelte-ignore a11y-no-static-element-interactions -->
<div
  class="spm-overlay"
  transition:fade|global={{ duration: 300, easing: cubicOut }}
  on:click={close}
>
  <div
    class="spm-modal"
    in:scale|global={{ start: 1.06, opacity: 0, duration: 350, easing: cubicOut }}
    out:scale|global={{ start: 1.06, opacity: 1, duration: 350, easing: cubicOut }}
    on:click|stopPropagation
  >
    <!-- Header -->
    <div class="spm-header">
      <div class="header-left">
        <h2>Subtitle Packs</h2>
        <span class="header-sub">{details?.title || details?.name || ""}</span>
      </div>
      <div class="header-actions">
        {#if Object.keys(coverage).length > 0}
          <button class="btn-clear" on:click={clearAll} title="Remove all subtitle packs">
            <i class="ri-delete-bin-line"></i>
            Clear All
          </button>
        {/if}
        <div class="import-wrap" class:open={showImportMenu}>
          <button
            class="btn-import"
            class:loading={importing}
            on:click={() => (showImportMenu = !showImportMenu)}
            disabled={importing}
          >
            {#if importing}
              <i class="ri-loader-4-line spin"></i>
              Importing…
            {:else}
              <i class="ri-download-2-line"></i>
              Import
              <i class="ri-arrow-down-s-line"></i>
            {/if}
          </button>
          {#if showImportMenu}
            <div class="import-menu">
              <button class="import-menu-item" on:click={importFiles}>
                <i class="ri-file-text-line"></i>
                <span>
                  <strong>Files</strong>
                  <small>.srt, .ass, .vtt, .zip</small>
                </span>
              </button>
              <button class="import-menu-item" on:click={importFolder}>
                <i class="ri-folder-open-line"></i>
                <span>
                  <strong>Folder</strong>
                  <small>Import an entire directory</small>
                </span>
              </button>
            </div>
          {/if}
        </div>
      </div>
    </div>

    <!-- Import result banner -->
    {#if importResult}
      <div class="result-banner" class:has-errors={importResult.errors.length > 0}>
        <div class="result-stats">
          {#if importResult.imported > 0}
            <span class="result-good">
              <i class="ri-check-line"></i>
              {importResult.imported} imported
            </span>
          {/if}
          {#if importResult.skipped > 0}
            <span class="result-skip">
              {importResult.skipped} skipped (no episode info found)
            </span>
          {/if}
          {#if importResult.errors.length > 0}
            <span class="result-err">
              <i class="ri-error-warning-line"></i>
              {importResult.errors.length} error{importResult.errors.length !== 1 ? "s" : ""}
            </span>
          {/if}
        </div>
        <button class="result-dismiss" on:click={() => (importResult = null)}>
          <i class="ri-close-line"></i>
        </button>
      </div>
    {/if}

    <!-- Content -->
    <div class="spm-content">
      {#if loading}
        <div class="loading-state">
          <div class="spinner"></div>
          <p>Loading…</p>
        </div>
      {:else}
        <!-- Hint -->
        <p class="hint">
          <i class="ri-information-line"></i>
          Subtitle files are matched by filename (S01E02, 1x02, E02…) and shown in the player's subtitle menu when watching an episode.
        </p>

        {#if details?.seasons}
          <div class="seasons-list">
            {#each details.seasons.filter(s => s.season_number > 0) as season}
              {@const sNum = season.season_number}
              {@const episodes = allSeasonsData[sNum]?.episodes || []}
              {@const subCount = seasonSubCount(sNum)}
              {@const total = episodes.length || season.episode_count}
              {@const isExpanded = expandedSeasons.has(sNum)}
              {@const isFull = total > 0 && subCount === total}

              <div class="season-row" class:full={isFull}>
                <button class="season-toggle" on:click={() => toggleSeason(sNum)}>
                  <div class="season-left">
                    <span class="season-label">Season {sNum}</span>
                    {#if subCount === 0}
                      <span class="badge none">{total > 0 ? total : season.episode_count} episodes — no subtitles</span>
                    {:else if isFull}
                      <span class="badge full">
                        <i class="ri-check-double-line"></i>
                        {subCount}/{total}
                      </span>
                    {:else}
                      <span class="badge partial">{subCount}/{total > 0 ? total : '?'} episodes</span>
                    {/if}
                  </div>
                  <i class="ri-arrow-down-s-line chevron" class:rotated={isExpanded}></i>
                </button>

                {#if isExpanded}
                  <div class="ep-list">
                    {#if episodes.length > 0}
                      {#each episodes as ep}
                        {@const hasSub = episodeHasSub(sNum, ep.episode_number)}
                        <div class="ep-row" class:has-sub={hasSub}>
                          <span class="ep-num">E{ep.episode_number}</span>
                          <span class="ep-name">{ep.name}</span>
                          {#if hasSub}
                            <span class="sub-chip">
                              <i class="ri-closed-captioning-fill"></i>
                              Subtitle
                            </span>
                            <button
                              class="btn-remove-ep"
                              title="Remove subtitle"
                              on:click={() => removeEpisode(sNum, ep.episode_number)}
                            >
                              <i class="ri-delete-bin-line"></i>
                            </button>
                          {:else}
                            <span class="no-sub">—</span>
                          {/if}
                        </div>
                      {/each}
                    {:else}
                      <!-- Episode list not yet loaded, show numbered placeholders -->
                      {#each Array(season.episode_count) as _, idx}
                        {@const epNum = idx + 1}
                        {@const hasSub = episodeHasSub(sNum, epNum)}
                        <div class="ep-row" class:has-sub={hasSub}>
                          <span class="ep-num">E{epNum}</span>
                          <span class="ep-name ep-name-placeholder">Episode {epNum}</span>
                          {#if hasSub}
                            <span class="sub-chip">
                              <i class="ri-closed-captioning-fill"></i>
                              Subtitle
                            </span>
                            <button
                              class="btn-remove-ep"
                              title="Remove subtitle"
                              on:click={() => removeEpisode(sNum, epNum)}
                            >
                              <i class="ri-delete-bin-line"></i>
                            </button>
                          {:else}
                            <span class="no-sub">—</span>
                          {/if}
                        </div>
                      {/each}
                    {/if}
                  </div>
                {/if}
              </div>
            {/each}
          </div>
        {/if}
      {/if}
    </div>
  </div>
</div>

<style>
  .spm-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.85);
    backdrop-filter: blur(12px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 10000;
  }

  .spm-modal {
    background: rgba(14, 14, 14, 0.98);
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: var(--border-radius-lg);
    width: 92%;
    max-width: 800px;
    max-height: 86vh;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    box-shadow: 0 24px 64px rgba(0, 0, 0, 0.7);
  }

  .spm-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 20px 24px 16px;
    border-bottom: 1px solid rgba(255, 255, 255, 0.07);
    flex-shrink: 0;
    gap: 16px;
  }

  .header-left {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .spm-header h2 {
    font-size: 18px;
    font-weight: 600;
    margin: 0;
    color: var(--text-primary);
    letter-spacing: -0.01em;
  }

  .header-sub {
    font-size: 12px;
    color: var(--text-tertiary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .header-actions {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
  }

  .btn-clear {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 7px 14px;
    background: rgba(239, 68, 68, 0.08);
    border: 1px solid rgba(239, 68, 68, 0.2);
    color: #f87171;
    font-size: 13px;
    font-weight: 500;
    border-radius: var(--border-radius-sm);
    cursor: pointer;
    transition: all 0.15s ease;
    font-family: inherit;
    white-space: nowrap;
  }

  .btn-clear:hover {
    background: rgba(239, 68, 68, 0.15);
    border-color: rgba(239, 68, 68, 0.35);
  }

  .import-wrap {
    position: relative;
  }

  .btn-import {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 16px;
    background: var(--accent-color);
    border: none;
    color: #000;
    font-size: 13px;
    font-weight: 600;
    border-radius: var(--border-radius-sm);
    cursor: pointer;
    transition: all 0.15s ease;
    font-family: inherit;
    white-space: nowrap;
  }

  .btn-import:hover:not(:disabled) {
    opacity: 0.88;
  }

  .btn-import:disabled {
    opacity: 0.6;
    cursor: default;
  }

  .btn-import i {
    font-size: 15px;
  }

  .import-menu {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    background: rgba(18, 18, 18, 0.98);
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: var(--border-radius-md);
    overflow: hidden;
    box-shadow: 0 12px 32px rgba(0, 0, 0, 0.5);
    z-index: 10;
    min-width: 200px;
  }

  .import-menu-item {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    padding: 12px 16px;
    background: transparent;
    border: none;
    color: var(--text-primary);
    cursor: pointer;
    text-align: left;
    font-family: inherit;
    transition: background 0.12s ease;
  }

  .import-menu-item:hover {
    background: rgba(255, 255, 255, 0.06);
  }

  .import-menu-item i {
    font-size: 18px;
    color: var(--text-tertiary);
    flex-shrink: 0;
  }

  .import-menu-item span {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .import-menu-item strong {
    font-size: 13px;
    font-weight: 600;
  }

  .import-menu-item small {
    font-size: 11px;
    color: var(--text-tertiary);
  }

  .result-banner {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 24px;
    background: rgba(46, 213, 115, 0.06);
    border-bottom: 1px solid rgba(46, 213, 115, 0.15);
    flex-shrink: 0;
  }

  .result-banner.has-errors {
    background: rgba(251, 191, 36, 0.05);
    border-bottom-color: rgba(251, 191, 36, 0.15);
  }

  .result-stats {
    display: flex;
    align-items: center;
    gap: 16px;
    font-size: 13px;
  }

  .result-good {
    display: flex;
    align-items: center;
    gap: 5px;
    color: #2ed573;
  }

  .result-skip {
    color: var(--text-tertiary);
  }

  .result-err {
    display: flex;
    align-items: center;
    gap: 5px;
    color: #fbbf24;
  }

  .result-dismiss {
    background: transparent;
    border: none;
    color: var(--text-tertiary);
    cursor: pointer;
    font-size: 16px;
    padding: 2px;
    display: flex;
    align-items: center;
  }

  .result-dismiss:hover {
    color: var(--text-primary);
  }

  .spm-content {
    flex: 1;
    overflow-y: auto;
    padding: 16px 24px 24px;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .loading-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 60px;
    gap: 14px;
    color: var(--text-secondary);
  }

  .loading-state p {
    margin: 0;
    font-size: 14px;
  }

  .spinner {
    width: 32px;
    height: 32px;
    border: 2px solid rgba(255, 255, 255, 0.08);
    border-top-color: var(--accent-color);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  .hint {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    font-size: 12px;
    color: var(--text-tertiary);
    margin: 0;
    line-height: 1.5;
  }

  .hint i {
    font-size: 14px;
    flex-shrink: 0;
    margin-top: 1px;
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

  .season-row.full {
    border-color: rgba(211, 118, 195, 0.25);
    background: rgba(211, 118, 195, 0.03);
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

  .season-left {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .season-label {
    font-size: 15px;
    font-weight: 600;
    color: var(--text-primary);
    white-space: nowrap;
  }

  .badge {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 12px;
    font-weight: 500;
    padding: 3px 10px;
    border-radius: 20px;
  }

  .badge.full {
    background: rgba(211, 118, 195, 0.12);
    color: var(--accent-color);
    border: 1px solid rgba(211, 118, 195, 0.3);
  }

  .badge.partial {
    background: rgba(251, 191, 36, 0.1);
    color: #fbbf24;
    border: 1px solid rgba(251, 191, 36, 0.25);
  }

  .badge.none {
    background: rgba(255, 255, 255, 0.05);
    color: var(--text-tertiary);
    border: 1px solid rgba(255, 255, 255, 0.08);
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

  .ep-list {
    border-top: 1px solid rgba(255, 255, 255, 0.05);
    display: flex;
    flex-direction: column;
  }

  .ep-row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 9px 18px;
    border-bottom: 1px solid rgba(255, 255, 255, 0.03);
    transition: background 0.1s ease;
  }

  .ep-row:last-child {
    border-bottom: none;
  }

  .ep-row:hover {
    background: rgba(255, 255, 255, 0.02);
  }

  .ep-num {
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

  .ep-row.has-sub .ep-name {
    color: var(--text-primary);
  }

  .ep-name-placeholder {
    color: var(--text-tertiary) !important;
    font-style: italic;
  }

  .sub-chip {
    display: flex;
    align-items: center;
    gap: 5px;
    background: rgba(211, 118, 195, 0.1);
    border: 1px solid rgba(211, 118, 195, 0.25);
    color: var(--accent-color);
    font-size: 11px;
    font-weight: 500;
    padding: 3px 10px;
    border-radius: 20px;
    flex-shrink: 0;
    white-space: nowrap;
  }

  .sub-chip i {
    font-size: 12px;
  }

  .no-sub {
    font-size: 13px;
    color: rgba(255, 255, 255, 0.15);
    flex-shrink: 0;
    margin-left: auto;
  }

  .btn-remove-ep {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    background: transparent;
    border: 1px solid transparent;
    color: rgba(239, 68, 68, 0.5);
    border-radius: var(--border-radius-sm);
    cursor: pointer;
    transition: all 0.15s ease;
    font-size: 14px;
    flex-shrink: 0;
  }

  .btn-remove-ep:hover {
    background: rgba(239, 68, 68, 0.1);
    border-color: rgba(239, 68, 68, 0.25);
    color: #f87171;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }
</style>
