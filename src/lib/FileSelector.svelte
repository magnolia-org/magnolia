<script>
    import { createEventDispatcher } from "svelte";
    import { fade, fly, slide } from "svelte/transition";
    import { cubicOut } from "svelte/easing";
    import { 
        findDifferingPatterns, 
        renderFilenameWithHighlights as renderHighlights, 
        getHighlightedNumber as getHighlight,
        formatFileSize,
        sortFiles,
        sortSelectionOrder
    } from "./fileSelector.js";

    export let files = [];
    export let showName = "";
    export let seasons = [];
    export let isMovie = false;

    const dispatch = createEventDispatcher();

    let sortOrder = "asc";
    let selectedFiles = new Set();
    let selectionOrder = [];
    let isDragging = false;
    let dragStartIndex = null;
    let assignments = {};
    let selectedSeason = 1;
    let selectedStartEpisode = null;
    let showPreviewModal = false;

    $: sortedFiles = sortFiles(files, sortOrder);
    $: highlightPatterns = findDifferingPatterns(sortedFiles.map(f => f.name));
    $: availableSeasons = seasons.filter(s => s.season_number > 0).map(s => s.season_number);
    $: if (availableSeasons.length > 0 && !availableSeasons.includes(selectedSeason)) {
        selectedSeason = availableSeasons[0];
    }
    $: currentSeasonData = seasons.find(s => s.season_number === selectedSeason);
    $: episodeCount = currentSeasonData?.episode_count || currentSeasonData?.episodes?.length || 24;
    
    $: hasSelection = selectedFiles.size > 0;
    $: assignedCount = Object.keys(assignments).length;
    $: canConfirm = isMovie ? selectedFiles.size === 1 : assignedCount > 0;

    $: assignedEpisodesForSeason = Object.values(assignments)
        .filter(a => a.season === selectedSeason)
        .map(a => a.episode);

    $: assignedFileIndices = new Set(Object.keys(assignments).map(k => parseInt(k)));
    $: sortedSelectionOrder = sortSelectionOrder(selectionOrder, files, sortOrder);

    $: previewAssignments = (() => {
        if (selectedStartEpisode === null || sortedSelectionOrder.length === 0) return {};
        const preview = {};
        let currentSeason = selectedSeason;
        let currentEpisode = selectedStartEpisode;
        
        for (const fileIndex of sortedSelectionOrder) {
            while (assignedEpisodesForSeason.includes(currentEpisode) || 
                   Object.values(assignments).some(a => a.season === currentSeason && a.episode === currentEpisode)) {
                currentEpisode++;
                if (currentEpisode > getEpisodeCountForSeason(currentSeason)) {
                    break;
                }
            }
            if (currentEpisode > getEpisodeCountForSeason(currentSeason)) break;
            preview[fileIndex] = { season: currentSeason, episode: currentEpisode };
            currentEpisode++;
        }
        return preview;
    })();

    $: previewOutOfRange = selectedStartEpisode !== null && 
        sortedSelectionOrder.length > 0 && 
        Object.keys(previewAssignments).length < sortedSelectionOrder.length;

    function renderFilenameWithHighlights(filename) {
        return renderHighlights(filename, highlightPatterns);
    }

    function getHighlightedNumber(filename) {
        return getHighlight(filename, highlightPatterns);
    }

    function toggleSort() {
        sortOrder = sortOrder === "asc" ? "desc" : "asc";
    }

    function handleFileClick(index, event) {
        if (event.button !== 0) return;
        event.preventDefault();
        
        const fileIndex = sortedFiles[index].index;
        
        if (isMovie) {
            selectedFiles = new Set([fileIndex]);
            selectionOrder = [fileIndex];
            return;
        }

        if (assignedFileIndices.has(fileIndex)) return;
        
        if (selectedFiles.has(fileIndex)) {
            const newSelection = new Set(selectedFiles);
            newSelection.delete(fileIndex);
            selectedFiles = newSelection;
            selectionOrder = selectionOrder.filter(i => i !== fileIndex);
        } else {
            isDragging = true;
            dragStartIndex = index;
            selectedFiles = new Set([...selectedFiles, fileIndex]);
            selectionOrder = [...selectionOrder, fileIndex];
        }
    }

    function handleMouseEnter(index) {
        if (!isDragging) return;
        const start = Math.min(dragStartIndex, index);
        const end = Math.max(dragStartIndex, index);
        
        const newSelection = new Set(selectedFiles);
        const newOrder = [...selectionOrder];
        
        for (let i = start; i <= end; i++) {
            const fileIndex = sortedFiles[i].index;
            if (!newSelection.has(fileIndex) && !assignedFileIndices.has(fileIndex)) {
                newSelection.add(fileIndex);
                newOrder.push(fileIndex);
            }
        }
        selectedFiles = newSelection;
        selectionOrder = newOrder;
    }

    function handleGlobalMouseUp() {
        isDragging = false;
    }

    function getEpisodeCountForSeason(seasonNum) {
        const season = seasons.find(s => s.season_number === seasonNum);
        return season?.episode_count || 99;
    }

    function selectStartEpisode(episodeNum) {
        if (assignedEpisodesForSeason.includes(episodeNum)) return;
        selectedStartEpisode = selectedStartEpisode === episodeNum ? null : episodeNum;
    }

    function assignSelectedFiles() {
        if (sortedSelectionOrder.length === 0 || selectedStartEpisode === null) return;
        
        const newAssignments = { ...assignments };
        let currentSeason = selectedSeason;
        let currentEpisode = selectedStartEpisode;
        
        for (const fileIndex of sortedSelectionOrder) {
            while (assignedEpisodesForSeason.includes(currentEpisode) || 
                   Object.values(newAssignments).some(a => a.season === currentSeason && a.episode === currentEpisode)) {
                currentEpisode++;
                if (currentEpisode > getEpisodeCountForSeason(currentSeason)) {
                    currentEpisode = 1;
                    currentSeason++;
                }
            }
            
            newAssignments[fileIndex] = { season: currentSeason, episode: currentEpisode };
            currentEpisode++;
            if (currentEpisode > getEpisodeCountForSeason(currentSeason)) {
                currentEpisode = 1;
                currentSeason++;
            }
        }
        
        assignments = newAssignments;
        clearSelection();
        selectedStartEpisode = null;
    }

    function clearSelection() {
        selectedFiles = new Set();
        selectionOrder = [];
    }

    function removeAssignment(fileIndex) {
        const newAssignments = { ...assignments };
        delete newAssignments[fileIndex];
        assignments = newAssignments;
    }

    function clearAllAssignments() {
        assignments = {};
        clearSelection();
    }

    function confirm() {
        if (isMovie) {
            if (selectedFiles.size === 1) {
                const fileIndex = [...selectedFiles][0];
                const file = files.find(f => f.index === fileIndex);
                dispatch("confirm", [{ file, season: 0, episode: 0 }]);
            }
            return;
        }

        const result = Object.entries(assignments).map(([fileIndex, assignment]) => {
            const file = files.find(f => f.index === parseInt(fileIndex));
            return { file, season: assignment.season, episode: assignment.episode };
        });
        dispatch("confirm", result);
    }

    function close() {
        dispatch("close");
    }

    function getFileForEpisode(season, episode) {
        const entry = Object.entries(assignments).find(([_, a]) => a.season === season && a.episode === episode);
        if (!entry) return null;
        return files.find(f => f.index === parseInt(entry[0]));
    }

    function getSortedAssignments() {
        return Object.entries(assignments)
            .map(([fileIndex, assignment]) => {
                const file = files.find(f => f.index === parseInt(fileIndex));
                return { file, ...assignment };
            })
            .sort((a, b) => {
                if (a.season !== b.season) return a.season - b.season;
                return a.episode - b.episode;
            });
    }
</script>

<svelte:window on:mouseup={handleGlobalMouseUp} />

<!-- svelte-ignore a11y-click-events-have-key-events -->
<!-- svelte-ignore a11y-no-static-element-interactions -->
<div class="modal-overlay" transition:fade={{ duration: 150 }} on:click={close}>
    <div class="modal-content" transition:fly={{ y: 30, duration: 250, easing: cubicOut }} on:click|stopPropagation>
        <div class="modal-header">
            <div class="header-title">
                <h3>Map Episode Files</h3>
                <span class="header-subtitle">{showName} • {files.length} files</span>
            </div>
        </div>

        <div class="main-area">
            <div class="file-list-section">
                <div class="section-header">
                    <span class="section-title">Files</span>
                    <div class="section-actions">
                        {#if hasSelection}
                            <button class="fs-action-btn deselect" on:click={clearSelection}>
                                <i class="ri-checkbox-indeterminate-line"></i>
                                Deselect
                            </button>
                        {/if}
                        <button class="fs-action-btn" on:click={toggleSort} title="Sort {sortOrder === 'asc' ? 'Z-A' : 'A-Z'}">
                            <i class="ri-sort-alphabet-{sortOrder === 'asc' ? 'asc' : 'desc'}"></i>
                            {sortOrder === 'asc' ? 'A-Z' : 'Z-A'}
                        </button>
                    </div>
                </div>
                <div class="file-list" class:dragging={isDragging}>
                    {#each sortedFiles as file, index (file.index)}
                        {@const isSelected = selectedFiles.has(file.index)}
                        {@const assignment = assignments[file.index]}
                        {@const isAssigned = !!assignment}
                        {@const previewAssignment = previewAssignments[file.index]}
                        {@const hasPreview = isSelected && !isAssigned && previewAssignment}
                        {@const filenameParts = renderFilenameWithHighlights(file.name)}
                        <div 
                            class="file-row"
                            class:selected={isSelected}
                            class:assigned={isAssigned}
                            class:has-preview={hasPreview}
                            on:mousedown={(e) => handleFileClick(index, e)}
                            on:mouseenter={() => handleMouseEnter(index)}
                        >
                            <div class="file-info">
                                <div class="file-name">
                                    {#each filenameParts as part}
                                        <span class:dimmed={!part.highlighted && highlightPatterns.length > 0} class:ep-number={part.highlighted}>{part.text}</span>
                                    {/each}
                                </div>
                                <div class="file-meta">
                                    <span class="file-size">{formatFileSize(file.length || file.size)}</span>
                                </div>
                            </div>
                            {#if isAssigned}
                                <div class="assignment-group">
                                    <span class="episode-tag">S{assignment.season}E{assignment.episode}</span>
                                    <button class="remove-btn" on:click|stopPropagation={() => removeAssignment(file.index)} title="Remove assignment">
                                        <i class="ri-close-line"></i>
                                    </button>
                                </div>
                            {:else if hasPreview && !isMovie}
                                <div class="assignment-group preview">
                                    <span class="episode-tag preview">S{previewAssignment.season}E{previewAssignment.episode}</span>
                                </div>
                            {/if}
                        </div>
                    {/each}
                </div>
            </div>

            {#if hasSelection && !isMovie}
                <div class="episode-picker-section" transition:slide={{ duration: 200, axis: 'x', easing: cubicOut }}>
                    <div class="section-header">
                        <span class="section-title">Assign to Episode</span>
                        <span class="selection-count">{sortedSelectionOrder.length} selected</span>
                    </div>
                    
                    <div class="season-tabs">
                        {#each availableSeasons as s}
                            <button 
                                class="season-tab" 
                                class:active={selectedSeason === s}
                                on:click={() => selectedSeason = s}
                            >
                                S{s}
                            </button>
                        {/each}
                    </div>
                    
                    <div class="episode-grid">
                        {#each Array(episodeCount) as _, i}
                            {@const epNum = i + 1}
                            {@const assignedFile = getFileForEpisode(selectedSeason, epNum)}
                            {@const isAssigned = !!assignedFile}
                            {@const isSelected = selectedStartEpisode === epNum}
                            {@const previewOffset = selectedStartEpisode !== null && epNum >= selectedStartEpisode ? epNum - selectedStartEpisode : -1}
                            {@const previewFile = previewOffset >= 0 && previewOffset < sortedSelectionOrder.length ? files.find(f => f.index === sortedSelectionOrder[previewOffset]) : null}
                            {@const showPreview = !isAssigned && previewFile}
                            <button 
                                class="episode-cell"
                                class:assigned={isAssigned}
                                class:start-episode={isSelected}
                                class:preview={showPreview && !isSelected}
                                disabled={isAssigned}
                                on:click={() => selectStartEpisode(epNum)}
                                title={isAssigned ? `Assigned: ${assignedFile.name}` : isSelected ? `Starting episode` : showPreview ? `Will assign: ${previewFile.name}` : `Select E${epNum} as start`}
                            >
                                <span class="ep-num">{epNum}</span>
                                {#if isAssigned}
                                    <span class="ep-file">{getHighlightedNumber(assignedFile.name) || '✓'}</span>
                                {:else if showPreview}
                                    <span class="ep-file preview">{getHighlightedNumber(previewFile.name) || '?'}</span>
                                {/if}
                            </button>
                        {/each}
                    </div>
                    
                    <div class="picker-actions">
                        {#if previewOutOfRange}
                            <div class="out-of-range-warning">
                                <i class="ri-error-warning-line"></i>
                                Not enough episodes remaining
                            </div>
                        {/if}
                        <button 
                            class="assign-btn" 
                            disabled={selectedStartEpisode === null || previewOutOfRange}
                            on:click={assignSelectedFiles}
                        >
                            <i class="ri-arrow-right-line"></i>
                            Assign {sortedSelectionOrder.length} file{sortedSelectionOrder.length !== 1 ? 's' : ''} from E{selectedStartEpisode || '?'}
                        </button>
                    </div>
                </div>
            {/if}
        </div>

        <div class="modal-footer">
            <div class="footer-left">
                {#if isMovie}
                    <span class="status-text">
                        <i class="ri-information-line"></i>
                        Select the movie file to play.
                    </span>
                {:else if assignedCount > 0}
                    <button class="status-text success clickable" on:click={() => showPreviewModal = true}>
                        <i class="ri-checkbox-circle-fill"></i>
                        {assignedCount} file{assignedCount !== 1 ? 's' : ''} mapped
                        <i class="ri-external-link-line"></i>
                    </button>
                    <button class="clear-btn" on:click={clearAllAssignments}>
                        <i class="ri-delete-bin-line"></i>
                        Clear All
                    </button>
                {:else}
                    <span class="status-text">
                        <i class="ri-information-line"></i>
                        Click and drag to select files and then assign them to episodes by clicking the starting episode on the right.
                    </span>
                {/if}
            </div>
            <div class="footer-right">
                <button class="btn-secondary" on:click={close}>Cancel</button>
                <button class="btn-primary" on:click={confirm} disabled={!canConfirm}>
                    <i class="ri-check-line"></i>
                    Confirm
                </button>
            </div>
        </div>
    </div>
</div>

{#if showPreviewModal}
    <!-- svelte-ignore a11y-click-events-have-key-events -->
    <!-- svelte-ignore a11y-no-static-element-interactions -->
    <div class="preview-overlay" transition:fade={{ duration: 100 }} on:click={() => showPreviewModal = false}>
        <div class="preview-modal" transition:fly={{ y: 20, duration: 200, easing: cubicOut }} on:click|stopPropagation>
            <div class="preview-header">
                <h4>Mapped Files Preview</h4>
                <button class="preview-close" on:click={() => showPreviewModal = false}>
                    <i class="ri-close-line"></i>
                </button>
            </div>
            <div class="preview-list">
                {#each getSortedAssignments() as item}
                    <div class="preview-row">
                        <span class="preview-episode">S{item.season}E{item.episode}</span>
                        <span class="preview-filename">{item.file?.name || 'Unknown'}</span>
                    </div>
                {/each}
            </div>
        </div>
    </div>
{/if}

<style>
    .modal-overlay {
        position: fixed;
        inset: 0;
        background: rgba(0, 0, 0, 0.85);
        display: flex;
        justify-content: center;
        align-items: center;
        z-index: 9800;
        backdrop-filter: blur(8px);
    }

    .modal-content {
        background: var(--bg-primary);
        width: 95%;
        max-width: 1100px;
        max-height: 85vh;
        border-radius: var(--border-radius-lg);
        border: 1px solid rgba(255, 255, 255, 0.08);
        display: flex;
        flex-direction: column;
        box-shadow: 0 25px 80px rgba(0, 0, 0, 0.6);
        overflow: hidden;
    }

    .modal-header {
        padding: var(--spacing-lg) var(--spacing-2xl);
        border-bottom: 1px solid rgba(255, 255, 255, 0.06);
        display: flex;
        justify-content: space-between;
        align-items: center;
        background: var(--bg-secondary);
        flex-shrink: 0;
    }

    .header-title h3 {
        margin: 0;
        font-size: 17px;
        font-weight: 600;
        color: var(--text-primary);
    }

    .header-subtitle {
        font-size: 12px;
        color: var(--text-tertiary);
        margin-top: 2px;
        display: block;
    }

    .main-area {
        flex: 1;
        display: flex;
        overflow: hidden;
        min-height: 0;
    }

    .file-list-section {
        flex: 1;
        display: flex;
        flex-direction: column;
        overflow: hidden;
        min-width: 0;
        transition: flex 0.2s ease;
    }

    .section-header {
        display: flex;
        justify-content: space-between;
        align-items: center;
        padding: var(--spacing-md) var(--spacing-xl);
        background: rgba(0, 0, 0, 0.2);
        border-bottom: 1px solid rgba(255, 255, 255, 0.04);
        flex-shrink: 0;
    }

    .section-title {
        font-size: 12px;
        font-weight: 600;
        color: var(--text-secondary);
        text-transform: uppercase;
        letter-spacing: 0.05em;
    }

    .section-actions {
        display: flex;
        align-items: center;
        gap: var(--spacing-sm);
    }

    .fs-action-btn {
        display: inline-flex;
        align-items: center;
        gap: 5px;
        padding: 5px 10px;
        background: rgba(255, 255, 255, 0.06);
        border: 1px solid rgba(255, 255, 255, 0.1);
        border-radius: var(--border-radius-sm);
        color: var(--text-secondary);
        font-size: 11px;
        font-weight: 500;
        cursor: pointer;
        transition: all 0.15s ease;
        font-family: inherit;
        white-space: nowrap;
    }

    .fs-action-btn:hover {
        background: rgba(255, 255, 255, 0.1);
        color: var(--text-primary);
    }

    .fs-action-btn i {
        font-size: 13px;
    }

    .fs-action-btn.deselect {
        background: rgba(211, 118, 195, 0.1);
        border-color: rgba(211, 118, 195, 0.2);
        color: var(--accent-color);
    }

    .fs-action-btn.deselect:hover {
        background: rgba(211, 118, 195, 0.15);
        border-color: rgba(211, 118, 195, 0.3);
    }

    .file-list {
        flex: 1;
        overflow-y: auto;
        padding: var(--spacing-xs);
        user-select: none;
    }

    .file-list.dragging {
        cursor: grabbing;
    }

    .file-row {
        display: flex;
        align-items: center;
        gap: var(--spacing-md);
        padding: var(--spacing-sm) var(--spacing-md);
        border-radius: var(--border-radius-sm);
        cursor: pointer;
        transition: background 0.1s ease;
        border: 1px solid transparent;
    }

    .file-row:hover {
        background: rgba(255, 255, 255, 0.04);
    }

    .file-row.selected {
        background: rgba(211, 118, 195, 0.12);
        border-color: rgba(211, 118, 195, 0.3);
    }

    .file-row.assigned {
        background: rgba(16, 185, 129, 0.06);
        cursor: default;
        opacity: 0.85;
    }

    .file-row.assigned:hover {
        background: rgba(16, 185, 129, 0.08);
    }

    .file-info {
        flex: 1;
        min-width: 0;
    }

    .file-name {
        font-size: 12px;
        font-family: "Geist Mono Variable", monospace;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
        color: var(--text-primary);
    }

    .file-name .dimmed {
        color: var(--text-tertiary);
    }

    .file-name .ep-number {
        color: var(--text-primary);
        font-weight: 700;
        background: rgba(211, 118, 195, 0.2);
        padding: 1px 4px;
        border-radius: 3px;
    }

    .file-meta {
        margin-top: 2px;
    }

    .file-size {
        font-size: 10px;
        color: var(--text-tertiary);
    }

    .assignment-group {
        display: flex;
        align-items: center;
        gap: 4px;
        flex-shrink: 0;
    }

    .episode-tag {
        padding: 4px 8px;
        background: rgba(16, 185, 129, 0.15);
        border-radius: var(--border-radius-sm);
        font-size: 11px;
        font-weight: 600;
        font-family: "Geist Mono Variable", monospace;
        color: #34d399;
    }

    .episode-tag.preview {
        background: rgba(211, 118, 195, 0.15);
        color: var(--accent-color);
        border: 1px dashed rgba(211, 118, 195, 0.4);
    }

    .assignment-group.preview {
        opacity: 0.85;
    }

    .remove-btn {
        width: 22px;
        height: 22px;
        display: flex;
        align-items: center;
        justify-content: center;
        background: transparent;
        border: none;
        border-radius: var(--border-radius-sm);
        color: var(--text-tertiary);
        cursor: pointer;
        transition: all 0.12s ease;
        opacity: 0;
    }

    .file-row:hover .remove-btn {
        opacity: 1;
    }

    .remove-btn:hover {
        background: rgba(239, 68, 68, 0.15);
        color: #ef4444;
    }

    .remove-btn i {
        font-size: 14px;
    }

    .episode-picker-section {
        width: 320px;
        flex-shrink: 0;
        display: flex;
        flex-direction: column;
        border-left: 1px solid rgba(255, 255, 255, 0.06);
        background: rgba(0, 0, 0, 0.15);
    }

    .selection-count {
        font-size: 11px;
        color: var(--accent-color);
        font-weight: 500;
    }

    .season-tabs {
        display: flex;
        gap: 4px;
        padding: var(--spacing-sm) var(--spacing-lg);
        background: rgba(0, 0, 0, 0.2);
        border-bottom: 1px solid rgba(255, 255, 255, 0.04);
        flex-wrap: wrap;
    }

    .season-tab {
        padding: 6px 12px;
        background: rgba(255, 255, 255, 0.05);
        border: 1px solid rgba(255, 255, 255, 0.08);
        border-radius: var(--border-radius-sm);
        color: var(--text-secondary);
        font-size: 11px;
        font-weight: 600;
        cursor: pointer;
        transition: all 0.15s ease;
        font-family: inherit;
    }

    .season-tab:hover {
        background: rgba(255, 255, 255, 0.08);
        color: var(--text-primary);
    }

    .season-tab.active {
        background: var(--accent-color);
        border-color: var(--accent-color);
        color: #000;
    }

    .episode-grid {
        flex: 1;
        display: grid;
        grid-template-columns: repeat(4, 1fr);
        gap: 6px;
        padding: var(--spacing-md);
        overflow-y: auto;
        align-content: start;
    }

    .episode-cell {
        aspect-ratio: 1;
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        gap: 2px;
        background: rgba(255, 255, 255, 0.04);
        border: 1px solid rgba(255, 255, 255, 0.08);
        border-radius: var(--border-radius-sm);
        cursor: pointer;
        transition: all 0.15s ease;
        font-family: inherit;
        padding: 4px;
    }

    .episode-cell:hover:not(:disabled) {
        background: rgba(211, 118, 195, 0.15);
        border-color: rgba(211, 118, 195, 0.4);
    }

    .episode-cell.assigned {
        background: rgba(16, 185, 129, 0.15);
        border-color: rgba(16, 185, 129, 0.3);
        cursor: not-allowed;
    }

    .episode-cell.start-episode {
        background: rgba(211, 118, 195, 0.25);
        border-color: var(--accent-color);
        border-width: 2px;
        box-shadow: 0 0 0 2px rgba(211, 118, 195, 0.15);
    }

    .episode-cell.preview {
        background: rgba(211, 118, 195, 0.1);
        border-color: rgba(211, 118, 195, 0.25);
        border-style: dashed;
    }

    .episode-cell:disabled {
        opacity: 0.7;
    }

    .ep-num {
        font-size: 13px;
        font-weight: 600;
        color: var(--text-secondary);
    }

    .episode-cell.assigned .ep-num {
        color: #34d399;
    }

    .episode-cell.start-episode .ep-num {
        color: var(--accent-color);
        font-weight: 700;
    }

    .episode-cell.preview .ep-num {
        color: var(--accent-color);
        opacity: 0.8;
    }

    .ep-file {
        font-size: 9px;
        font-family: "Geist Mono Variable", monospace;
        color: #34d399;
        max-width: 100%;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .ep-file.preview {
        color: var(--accent-color);
        opacity: 0.7;
    }

    .picker-actions {
        padding: var(--spacing-md);
        border-top: 1px solid rgba(255, 255, 255, 0.04);
        display: flex;
        flex-direction: column;
        gap: var(--spacing-sm);
    }

    .out-of-range-warning {
        display: flex;
        align-items: center;
        justify-content: center;
        gap: 6px;
        padding: 8px 12px;
        background: rgba(239, 68, 68, 0.1);
        border: 1px solid rgba(239, 68, 68, 0.2);
        border-radius: var(--border-radius-sm);
        color: #ef4444;
        font-size: 11px;
        font-weight: 500;
    }

    .out-of-range-warning i {
        font-size: 14px;
    }

    .assign-btn {
        width: 100%;
        display: flex;
        align-items: center;
        justify-content: center;
        gap: 8px;
        padding: 10px 16px;
        background: var(--accent-color);
        border: none;
        border-radius: var(--border-radius-sm);
        color: #000;
        font-size: 12px;
        font-weight: 600;
        cursor: pointer;
        transition: all 0.15s ease;
        font-family: inherit;
    }

    .assign-btn:hover:not(:disabled) {
        filter: brightness(1.1);
    }

    .assign-btn:disabled {
        opacity: 0.4;
        cursor: not-allowed;
    }

    .assign-btn i {
        font-size: 15px;
    }

    .modal-footer {
        display: flex;
        justify-content: space-between;
        align-items: center;
        padding: var(--spacing-md) var(--spacing-xl);
        background: rgba(0, 0, 0, 0.3);
        border-top: 1px solid rgba(255, 255, 255, 0.06);
        flex-shrink: 0;
    }

    .footer-left {
        display: flex;
        align-items: center;
        gap: var(--spacing-md);
    }

    .footer-right {
        display: flex;
        gap: var(--spacing-sm);
    }

    .status-text {
        display: flex;
        align-items: center;
        gap: 6px;
        font-size: 12px;
        color: var(--text-tertiary);
    }

    .status-text.success {
        color: #34d399;
    }

    .status-text.clickable {
        cursor: pointer;
        padding: 6px 10px;
        border-radius: var(--border-radius-sm);
        background: transparent;
        border: none;
        font-family: inherit;
        transition: background 0.15s ease;
    }

    .status-text.clickable:hover {
        background: rgba(16, 185, 129, 0.1);
    }

    .status-text.clickable .ri-external-link-line {
        font-size: 12px;
        opacity: 0.6;
        margin-left: 2px;
    }

    .status-text i {
        font-size: 14px;
    }

    .preview-overlay {
        position: fixed;
        inset: 0;
        background: rgba(0, 0, 0, 0.7);
        display: flex;
        justify-content: center;
        align-items: center;
        z-index: 9900;
    }

    .preview-modal {
        background: var(--bg-primary);
        border-radius: var(--border-radius-lg);
        border: 1px solid rgba(255, 255, 255, 0.1);
        width: 90%;
        max-width: 500px;
        max-height: 60vh;
        display: flex;
        flex-direction: column;
        box-shadow: 0 20px 60px rgba(0, 0, 0, 0.5);
        overflow: hidden;
    }

    .preview-header {
        display: flex;
        justify-content: space-between;
        align-items: center;
        padding: var(--spacing-md) var(--spacing-lg);
        border-bottom: 1px solid rgba(255, 255, 255, 0.06);
        background: var(--bg-secondary);
    }

    .preview-header h4 {
        margin: 0;
        font-size: 14px;
        font-weight: 600;
        color: var(--text-primary);
    }

    .preview-close {
        width: 28px;
        height: 28px;
        display: flex;
        align-items: center;
        justify-content: center;
        background: transparent;
        border: none;
        border-radius: var(--border-radius-sm);
        color: var(--text-tertiary);
        cursor: pointer;
        transition: all 0.15s ease;
    }

    .preview-close:hover {
        background: rgba(255, 255, 255, 0.1);
        color: var(--text-primary);
    }

    .preview-close i {
        font-size: 18px;
    }

    .preview-list {
        flex: 1;
        overflow-y: auto;
        padding: var(--spacing-sm);
    }

    .preview-row {
        display: flex;
        align-items: center;
        gap: var(--spacing-md);
        padding: var(--spacing-sm) var(--spacing-md);
        border-radius: var(--border-radius-sm);
    }

    .preview-row:hover {
        background: rgba(255, 255, 255, 0.03);
    }

    .preview-episode {
        flex-shrink: 0;
        padding: 4px 8px;
        background: rgba(16, 185, 129, 0.15);
        border-radius: var(--border-radius-sm);
        font-size: 11px;
        font-weight: 600;
        font-family: "Geist Mono Variable", monospace;
        color: #34d399;
    }

    .preview-filename {
        flex: 1;
        font-size: 12px;
        font-family: "Geist Mono Variable", monospace;
        color: var(--text-secondary);
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }

    .clear-btn {
        display: inline-flex;
        align-items: center;
        gap: 5px;
        padding: 6px 10px;
        background: rgba(239, 68, 68, 0.1);
        border: 1px solid rgba(239, 68, 68, 0.2);
        border-radius: var(--border-radius-sm);
        color: #ef4444;
        font-size: 11px;
        font-weight: 500;
        cursor: pointer;
        transition: all 0.15s ease;
        font-family: inherit;
        white-space: nowrap;
    }

    .clear-btn:hover {
        background: rgba(239, 68, 68, 0.2);
        border-color: rgba(239, 68, 68, 0.3);
    }

    .clear-btn i {
        font-size: 13px;
    }

    .btn-secondary {
        padding: 8px 16px;
        background: rgba(255, 255, 255, 0.06);
        border: 1px solid rgba(255, 255, 255, 0.1);
        border-radius: var(--border-radius-sm);
        color: var(--text-secondary);
        font-size: 13px;
        font-weight: 500;
        cursor: pointer;
        transition: all 0.15s ease;
        font-family: inherit;
    }

    .btn-secondary:hover {
        background: rgba(255, 255, 255, 0.1);
        color: var(--text-primary);
    }

    .btn-primary {
        display: flex;
        align-items: center;
        gap: 6px;
        padding: 8px 16px;
        background: var(--accent-color);
        border: none;
        border-radius: var(--border-radius-sm);
        color: #000;
        font-size: 13px;
        font-weight: 600;
        cursor: pointer;
        transition: all 0.15s ease;
        font-family: inherit;
    }

    .btn-primary:hover:not(:disabled) {
        filter: brightness(1.1);
    }

    .btn-primary:disabled {
        opacity: 0.4;
        cursor: not-allowed;
    }

    .btn-primary i {
        font-size: 15px;
    }
</style>
