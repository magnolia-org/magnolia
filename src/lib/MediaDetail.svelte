<script>
  import { onMount, onDestroy } from "svelte";
  import {
    getMovieDetails,
    getTVDetails,
    getSeasonDetails,
    getMovieCredits,
    getTVCredits,
    getMovieRecommendations,
    getTVRecommendations,
    getImageUrl,
    getTVExternalIds,
    getMovieExternalIds,
  } from "./tmdb.js";
  import { myListStore } from "./stores/listStore.js";
  import { watchProgressStore } from "./stores/watchProgressStore.js";
  import { getTrackerPreference, setTrackerPreference } from "./stores/watchHistoryStore.js";
  import { invoke } from "@tauri-apps/api/core";
  import TorrentSelector from "./TorrentSelector.svelte";
  import FileSelector from "./FileSelector.svelte";
  import ErrorModal from "./ErrorModal.svelte";
  import TorrentManager from "./TorrentManager.svelte";

  import { createEventDispatcher } from "svelte";

  export let media = null;

  const dispatch = createEventDispatcher();

  let details = null;
  let loading = true;
  let backdropColor = "#1a1a1a";
  let prominentColor = "#1a1a1a";
  let textColor = "#ffffff";
  let selectedSeason = null;
  let seasonDetails = null;
  let allSeasonsData = {};
  let selectedEpisode = null;
  let credits = null;
  let recommendations = [];
  let activeTab = "";
  let availableTabs = [];
  let viewMode = "list"; // 'list' or 'grid'
  let episodeSearchQuery = "";

  // Torrent Search State
  let showTorrentSelector = false;
  let searchResults = [];
  let isSearching = false;
  let currentSearchQuery = "";
  let originalSearchQuery = ""; // The original auto-generated query for revert
  let pendingPlayRequest = null; // { season, episode }
  let currentImdbId = null; // IMDB ID for EZTV search
  
  // Manual file selection state
  let showFileSelector = false;
  let availableFiles = [];
  let selectedTorrentForManual = null;
  let selectedTorrentName = ""; // For display in TorrentSelector
  let manualHandleId = null;
  let autoPlayTriggered = false;

  // Error modal state
  let showErrorModal = false;
  let errorMessage = "";
  let errorTitle = "Error";

  // Torrent manager state
  let showTorrentManager = false;
  let showMoreMenu = false;
  let torrentManagerRefresh = 0;

  $: {
    if (media) {
      // Ensure media_type is set
      if (!media.media_type) {
        // If it has 'name' field (TMDB TV show), it's tv, otherwise movie
        media.media_type = media.name && !media.title ? "tv" : "movie";
      }
    }
  }

  $: isInMyList =
    media &&
    $myListStore.some(
      (item) => item.id === media.id && item.media_type === media.media_type,
    );

  // Helper function to detect anime (Animation genre ID is 16 in TMDB)
  const isAnime = () => {
    if (!details || !details.genres) return false;
    return details.genres.some(genre => genre.id === 16);
  };

  // Helper function to extract torrent name from magnet link
  function extractTorrentNameFromMagnet(magnetLink) {
    if (!magnetLink) return "";
    const dnMatch = magnetLink.match(/dn=([^&]+)/);
    if (dnMatch) {
      return decodeURIComponent(dnMatch[1].replace(/\+/g, ' '));
    }
    return "";
  }

  function hasMatchingEpisodes(seasonNumber, query) {
    if (!query) return true;
    const seasonData = allSeasonsData[seasonNumber];
    if (!seasonData || !seasonData.episodes) return false;
    return seasonData.episodes.some(e => 
      e.name.toLowerCase().includes(query.toLowerCase()) || 
      e.episode_number.toString().includes(query)
    );
  }

  $: {
    if (media && isInMyList !== undefined) {
      console.log(
        "🎥 MediaDetail: isInMyList updated for",
        media.title || media.name,
        ":",
        isInMyList,
      );
    }
  }

  $: if (media) {
    loadDetails();
    selectedSeason = null;
    seasonDetails = null;
    allSeasonsData = {};
    selectedEpisode = null;
    recommendations = [];
    autoPlayTriggered = false; // Reset autoplay flag for new media
    selectedTorrentName = ""; // Reset torrent name for new media
  }

  $: if (details) {
    availableTabs = [];
    if (details.seasons && details.seasons.length > 0) {
      availableTabs.push("seasons");
    }
    availableTabs.push("cast");
    availableTabs.push("details");

    if (details.seasons && details.seasons.length > 0) {
      activeTab = "seasons";
      // Skip specials (season 0) when selecting default
      const firstSeason = details.seasons.find((s) => s.season_number > 0);
      if (firstSeason) {
        selectedSeason = firstSeason.season_number;
      }
      loadAllSeasons();
    } else {
      activeTab = "cast";
    }

    loadRecommendations();
  }

  $: if (selectedSeason && details) {
    loadSeasonDetails();
  }

  onMount(() => {
    if (media && media.autoPlay && !autoPlayTriggered) {
      autoPlayTriggered = true;
      setTimeout(() => {
        if (details) {
          handleAutoPlay();
        }
      }, 500);
    }
    
    const handleKeyDown = (e) => {
      if (e.target.tagName === "INPUT" || e.target.tagName === "TEXTAREA")
        return;

      if (e.key === "Escape" && selectedEpisode) {
        e.preventDefault();
        e.stopPropagation();
        selectedEpisode = null;
      }
    };

    const handleClickOutside = (e) => {
      if (showMoreMenu && !e.target.closest('.more-menu-container')) {
        showMoreMenu = false;
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    window.addEventListener("click", handleClickOutside);

    return () => {
      window.removeEventListener("keydown", handleKeyDown);
      window.removeEventListener("click", handleClickOutside);
    };
  });

  async function loadDetails() {
    loading = true;
    details = null; // Clear previous details to prevent stale data
    try {
      if (media.media_type === "movie") {
        details = await getMovieDetails(media.id);
        credits = await getMovieCredits(media.id);
      } else {
        details = await getTVDetails(media.id);
        credits = await getTVCredits(media.id);
      }

      if (details && details.backdrop_path) {
        await extractColors(getImageUrl(details.backdrop_path, "w300"));
      }
    } catch (err) {
      console.error("Error loading details:", err);
      details = null; // Ensure details is null on error
    }
    loading = false;
  }

  async function loadSeasonDetails() {
    try {
      seasonDetails = await getSeasonDetails(details.id, selectedSeason);
    } catch (err) {
      console.error("Error loading season:", err);
    }
  }

  async function loadAllSeasons() {
    if (!details || !details.seasons) return;

    try {
      const seasonPromises = details.seasons
        .filter((s) => s.season_number > 0)
        .map(async (season) => {
          const data = await getSeasonDetails(details.id, season.season_number);
          return { seasonNumber: season.season_number, data };
        });

      const results = await Promise.all(seasonPromises);
      const seasonsData = {};
      results.forEach(({ seasonNumber, data }) => {
        seasonsData[seasonNumber] = data;
      });
      allSeasonsData = seasonsData;
    } catch (err) {
      console.error("Error loading all seasons:", err);
    }
  }

  async function loadRecommendations() {
    try {
      let response;
      if (media.media_type === "movie") {
        response = await getMovieRecommendations(media.id);
      } else {
        response = await getTVRecommendations(media.id);
      }
      recommendations = (response?.results?.slice(0, 10) || []).map((rec) => {
        // Ensure media_type is set for recommendations
        if (!rec.media_type) {
          rec.media_type = media.media_type;
        }
        return rec;
      });
    } catch (err) {
      console.error("Error loading recommendations:", err);
    }
  }

  async function extractColors(imageUrl) {
    try {
      const img = new Image();
      img.crossOrigin = "Anonymous";
      img.src = imageUrl;
      await new Promise((resolve, reject) => {
        img.onload = resolve;
        img.onerror = reject;
      });

      const canvas = document.createElement("canvas");
      const ctx = canvas.getContext("2d");
      canvas.width = img.width;
      canvas.height = img.height;
      ctx.drawImage(img, 0, 0);

      const imageData = ctx.getImageData(
        0,
        0,
        canvas.width,
        canvas.height,
      ).data;

      // Build a histogram of mid-range brightness pixels to find a dominant accent color
      const colorMap = {};
      for (let i = 0; i < imageData.length; i += 4 * 10) {
        const r = imageData[i];
        const g = imageData[i + 1];
        const b = imageData[i + 2];
        const brightness = (r + g + b) / 3;

        if (brightness > 40 && brightness < 200) {
          const colorKey = `${Math.floor(r / 20) * 20},${Math.floor(g / 20) * 20},${Math.floor(b / 20) * 20}`;
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
        const [r, g, b] = dominantColor.split(",").map(Number);
        prominentColor = `rgb(${r}, ${g}, ${b})`;

        const hexColor = rgbToHex(r, g, b);
        window.dispatchEvent(
          new CustomEvent("updateTitleBarColor", {
            detail: { color: hexColor },
          }),
        );
        textColor = "#ffffff";
      }

      // Build a muted backdrop by averaging mid-range brightness values
      let br = 0,
        bg = 0,
        bb = 0,
        count = 0;
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
      console.error("Color extraction failed:", err);
    }
  }

  function rgbToHex(r, g, b) {
    return (
      "#" +
      [r, g, b]
        .map((x) => {
          const hex = x.toString(16);
          return hex.length === 1 ? "0" + hex : hex;
        })
        .join("")
    );
  }

  function toggleMyList() {
    console.log(
      "🎥 MediaDetail: Toggle for:",
      media.title || media.name,
      "Current state:",
      isInMyList,
    );
    myListStore.toggleItem(media);
  }

  function formatRuntime(minutes) {
    if (!minutes) return "N/A";
    const hours = Math.floor(minutes / 60);
    const mins = minutes % 60;
    return `${hours}h ${mins}m`;
  }

  function formatDate(dateStr) {
    if (!dateStr) return "N/A";
    return new Date(dateStr).toLocaleDateString("en-US", {
      year: "numeric",
      month: "long",
      day: "numeric",
    });
  }

  function formatMoney(amount) {
    if (!amount) return "N/A";
    return new Intl.NumberFormat("en-US", {
      style: "currency",
      currency: "USD",
      minimumFractionDigits: 0,
    }).format(amount);
  }

  import { getRatingClass } from "./utils/colorUtils.js";
  import { formatTime } from "./utils/timeUtils.js";

  function showError(message, title = "Error") {
    errorMessage = message;
    errorTitle = title;
    showErrorModal = true;
  }

  // Get resume info for the play button
  function getResumeInfo() {
    if (!details) return null;
    const progress = watchProgressStore.getProgress(details.id, media.media_type);
    if (!progress) return null;
    
    const isMovie = media.media_type === 'movie' || !!details.title;
    
    if (isMovie) {
      // For movies, show timestamp if we have progress
      if (progress.currentTimestamp && progress.currentTimestamp > 60) {
        // Check if finished (>95%)
        const isFinished = progress.duration && (progress.currentTimestamp / progress.duration > 0.95);
        if (isFinished) return null; // Don't show resume if finished

        return {
          label: `Resume from ${formatTime(progress.currentTimestamp)}`,
          season: null,
          episode: null,
          timestamp: progress.currentTimestamp
        };
      }
    } else {
      // For TV shows, show last episode watched OR next episode if finished
      if (progress.currentSeason && progress.currentEpisode) {
        let season = progress.currentSeason;
        let episode = progress.currentEpisode;
        let timestamp = progress.currentTimestamp || 0;
        let label = `S${season}E${episode}`;
        
        // Check if finished (>90%)
        const isFinished = progress.duration && (progress.currentTimestamp / progress.duration > 0.9);
        
        if (isFinished && details.seasons) {
            // Find next episode
            const currentSeasonInfo = details.seasons.find(s => s.season_number === season);
            
            if (currentSeasonInfo) {
                if (episode < currentSeasonInfo.episode_count) {
                    episode++;
                    timestamp = 0;
                    label = `S${season}E${episode}`;
                } else {
                    // Next season?
                    const nextSeason = details.seasons.find(s => s.season_number === season + 1);
                    if (nextSeason) {
                        season++;
                        episode = 1;
                        timestamp = 0;
                        label = `S${season}E${episode}`;
                    }
                }
            }
        }
        
        const hasTimestamp = timestamp > 60;
        return {
          label: `${label}${hasTimestamp ? ` • ${formatTime(timestamp)}` : ''}`,
          season: season,
          episode: episode,
          timestamp: timestamp
        };
      }
    }
    return null;
  }

  // Reactive variable for resume info
  $: resumeInfo = details ? getResumeInfo() : null;

  function toggleSeason(seasonNumber) {
    if (selectedSeason === seasonNumber) {
      selectedSeason = null;
    } else {
      selectedSeason = seasonNumber;
    }
  }

  async function handleAutoPlay() {
    console.log('Auto-play triggered');
    
    // Clear autoPlay flag immediately to prevent re-triggering
    if (media && media.autoPlay) {
      media.autoPlay = false;
    }
    
    // Use resumeProgress if passed from quick play, otherwise load from store
    const progress = media.resumeProgress || watchProgressStore.getProgress(media.id, media.media_type);
    
    if (media.media_type === 'movie') {
      // Movie: play from beginning (timestamp resume handled by VideoPlayer)
      await handlePlay(0, 0);
    } else if (progress && progress.currentSeason && progress.currentEpisode) {
      // TV Show: resume from last watched episode
      console.log(`Resuming from S${progress.currentSeason}E${progress.currentEpisode} at ${progress.currentTimestamp}s`);
      await handlePlay(progress.currentSeason, progress.currentEpisode);
    } else {
      // TV Show: start from S01E01
      await handlePlay(1, 1);
    }
  }

  async function handlePlay(seasonNum, episodeNum, forceReselect = false) {
    console.log(
      "Play requested:",
      seasonNum,
      episodeNum,
      "Force reselect:",
      forceReselect,
    );
    pendingPlayRequest = { season: seasonNum, episode: episodeNum };

    // 1. Check persistence (skip if forcing reselect)
    if (!forceReselect) {
      try {
        const saved = await invoke("get_saved_selection", {
          showId: details.id,
          season: seasonNum,
          episode: episodeNum,
        });

        if (saved) {
          console.log("Found saved torrent:", saved);
          const success = await startStream(saved.magnet_link, saved.file_index);
          if (!success) {
             // If startStream failed (and cleared selection), we should probably stop here
             // The user saw an error modal.
             // If they click Play again, it will fall through to selector.
             return;
          }
          return;
        }
      } catch (err) {
        console.error("Error checking saved selection:", err);
      }
    }

    // 2. Start Search
    isSearching = true;
    showTorrentSelector = true;
    searchResults = [];

    // Try to get the saved torrent name for display
    if (!selectedTorrentName) {
      try {
        // Check if there's any saved selection for this show to get the torrent name
        const anySaved = await invoke("get_saved_selection", {
          showId: details.id,
          season: seasonNum,
          episode: episodeNum,
        });
        if (anySaved?.magnet_link) {
          selectedTorrentName = extractTorrentNameFromMagnet(anySaved.magnet_link);
        }
      } catch (err) {
        // Ignore - just won't show a previous torrent name
      }
    }

    const showName = details.title || details.name;
    const isMovie = media.media_type === "movie" || !!details.title;

    // Build search query - just the name, no season/episode
    let searchQuery = showName;
    if (isMovie) {
      const year = (details.release_date || "").split("-")[0];
      if (year) {
        searchQuery = `${showName} ${year}`;
      }
      currentSearchQuery = searchQuery;
      originalSearchQuery = searchQuery;
    } else {
      currentSearchQuery = showName;
      originalSearchQuery = showName;
    }

    console.log("Starting search:", searchQuery);

    // Detect if this is anime based on genre
    const mediaType = isAnime() ? "anime" : (isMovie ? "movie" : "tv");
    console.log("Media type for search:", mediaType, "Genres:", details.genres);

    // Get tracker preference
    const storedTrackers = getTrackerPreference();
    const trackerArray = Array.isArray(storedTrackers) && storedTrackers.length > 0 ? storedTrackers : null;
    console.log("Tracker preference:", trackerArray);

    // Fetch IMDB ID for EZTV support (TV shows only)
    let imdbId = null;
    if (!isMovie && (!trackerArray || trackerArray.includes('eztv'))) {
      try {
        const externalIds = await getTVExternalIds(details.id);
        if (externalIds?.imdb_id) {
          imdbId = externalIds.imdb_id;
          currentImdbId = imdbId;
          console.log("Got IMDB ID:", imdbId);
        }
      } catch (err) {
        console.warn("Failed to get IMDB ID:", err);
      }
    } else if (isMovie) {
      try {
        const externalIds = await getMovieExternalIds(details.id);
        if (externalIds?.imdb_id) {
          imdbId = externalIds.imdb_id;
          currentImdbId = imdbId;
          console.log("Got Movie IMDB ID:", imdbId);
        }
      } catch (err) {
        console.warn("Failed to get Movie IMDB ID:", err);
      }
    }

    // Execute search with filtering on backend
    try {
      searchResults = await invoke("search_nyaa_filtered", {
        query: searchQuery,
        season: isMovie ? null : seasonNum,
        episode: isMovie ? null : episodeNum,
        isMovie: isMovie,
        mediaType: mediaType,
        trackerPreference: trackerArray,
        imdbId: imdbId,
      });

      if (searchResults.length === 0) {
        console.log("No results found.");
      }
    } catch (err) {
      console.error("Search error:", err);
      searchResults = [];
    } finally {
      isSearching = false;
    }
  }

  function reselectTorrent() {
    if (details.seasons && details.seasons.length > 0) {
      if (selectedEpisode) {
        handlePlay(selectedSeason, selectedEpisode.episode_number, true);
      } else {
        handlePlay(1, 1, true);
      }
    } else {
      handlePlay(0, 0, true);
    }
  }

  function handleTorrentManagerSelect(event) {
    const { season, episode } = event.detail;
    showTorrentManager = false;
    handlePlay(season, episode, true);
  }

  function handleTorrentManagerClose() {
    showTorrentManager = false;
  }

  async function onTorrentSelect(event) {
    const torrent = event.detail;
    console.log("Selected torrent:", torrent);
    
    // Store the selected torrent name for display
    selectedTorrentName = torrent.title || "";

    if (!pendingPlayRequest) return;

    // We need to find the right file in the torrent.
    // For now, we'll add the torrent in "list_only" mode (via add_torrent) to get file list,
    // then try to match the episode.

    try {
      // 1. Add torrent to get metadata
      // Note: add_torrent returns a handle_id
      const handleId = await invoke("add_torrent", {
        magnetOrUrl: torrent.magnet_link,
      });
      const info = await invoke("get_torrent_info", { handleId });

      console.log("Torrent info:", info);
      
      // Filter to video files only
      const videoFiles = info.files.filter(f => {
        const ext = f.name.toLowerCase();
        return ext.endsWith('.mkv') || ext.endsWith('.mp4') || 
               ext.endsWith('.avi') || ext.endsWith('.mov') || 
               ext.endsWith('.webm') || ext.endsWith('.m4v');
      });

      // 2. Find the matching file for the requested episode
      const s = pendingPlayRequest.season.toString().padStart(2, "0");
      const e = pendingPlayRequest.episode.toString().padStart(2, "0");

      let fileIndex = -1;

      // Try specific match first
      fileIndex = videoFiles.findIndex(
        (f) =>
          f.name.toUpperCase().includes(`S${s}E${e}`) ||
          f.name.toUpperCase().includes(`${pendingPlayRequest.season}X${e}`),
      );

      // If not found, and it's a single video file torrent, use it
      if (fileIndex === -1 && videoFiles.length === 1) {
        fileIndex = 0;
      }

      // If still not found, maybe try just episode number if it's a season pack?
      if (fileIndex === -1) {
        fileIndex = videoFiles.findIndex(
          (f) =>
            f.name.toUpperCase().includes(`E${e}`) ||
            f.name.toUpperCase().includes(` ${e} `),
        );
      }

      // Check if this is a movie
      const isMovie = media.media_type === 'movie' || !!details.title;
      
      if (fileIndex !== -1) {
        console.log("Found matching file at index:", fileIndex);
        const matchedFile = videoFiles[fileIndex];

        // 3. Save selection for the requested episode
        await invoke("save_torrent_selection", {
          showId: details.id,
          season: pendingPlayRequest.season,
          episode: pendingPlayRequest.episode,
          magnetLink: torrent.magnet_link,
          fileIndex: matchedFile.index,
        });

        torrentManagerRefresh++;

        // 4. Auto-assign ALL other video files that have episode numbers
        if (videoFiles.length > 1 && !isMovie) {
          console.log("Multi-episode torrent detected, auto-assigning all episodes");
          for (const file of videoFiles) {
            if (file.index === matchedFile.index) continue; // Skip the one we already saved
            
            // Try multiple patterns to extract episode info
            const filename = file.name;
            let season = pendingPlayRequest.season; // Default to current season
            let episode = null;
            
            // Pattern: S01E05, S1E5
            const sxeMatch = filename.match(/S(\d{1,2})E(\d{1,3})/i);
            if (sxeMatch) {
              season = parseInt(sxeMatch[1]);
              episode = parseInt(sxeMatch[2]);
            }
            
            // Pattern: 1x05, 01x05
            if (!episode) {
              const xMatch = filename.match(/(\d{1,2})x(\d{2,3})/i);
              if (xMatch) {
                season = parseInt(xMatch[1]);
                episode = parseInt(xMatch[2]);
              }
            }
            
            // Pattern: Episode 5, Ep 05, E05 (without season)
            if (!episode) {
              const epMatch = filename.match(/(?:Episode|Ep\.?|E)[\s._-]*(\d{1,3})/i);
              if (epMatch) {
                episode = parseInt(epMatch[1]);
              }
            }
            
            // Pattern: - 05 - or [ 05 ] or common anime patterns like "- 05 "
            if (!episode) {
              const dashMatch = filename.match(/[-\[\s](\d{2,3})[-\]\s]/);
              if (dashMatch) {
                episode = parseInt(dashMatch[1]);
              }
            }
            
            if (episode && episode > 0) {
              console.log(`Auto-assigning S${season}E${episode} to file: ${filename}`);
              await invoke("save_torrent_selection", {
                showId: details.id,
                season: season,
                episode: episode,
                magnetLink: torrent.magnet_link,
                fileIndex: file.index,
              });
            }
          }
        }

        // 5. Start Stream
        startStream(torrent.magnet_link, matchedFile.index, handleId);
        showTorrentSelector = false;
      } else {
        // For movies with a single file, auto-select it without showing selector
        if (isMovie && videoFiles.length === 1) {
          console.log("Movie with single file, auto-selecting");
          const singleFile = videoFiles[0];
          
          await invoke("save_torrent_selection", {
            showId: details.id,
            season: pendingPlayRequest.season,
            episode: pendingPlayRequest.episode,
            magnetLink: torrent.magnet_link,
            fileIndex: singleFile.index,
          });
          
          torrentManagerRefresh++;
          
          startStream(torrent.magnet_link, singleFile.index, handleId);
          showTorrentSelector = false;
        } else {
          // Show manual file selection for TV shows or movies with multiple files
          showManualFileSelector(torrent, info, handleId);
        }
      }
    } catch (err) {
      console.error("Error processing selection:", err);
      showError("Failed to load torrent metadata. Please try again.");
    }
  }

  async function startStream(magnetLink, fileIndex, existingHandleId = null) {
    try {
      let handleId = existingHandleId;
      if (handleId === null) {
        handleId = await invoke("add_torrent", { magnetOrUrl: magnetLink });
      }

      // Don't update progress here - let VideoPlayer handle it to preserve saved timestamps
      // The VideoPlayer will track progress during playback

      // Don't start stream here. Let VideoPlayer handle it so it can show loading screen.
      console.log("Opening player for handle:", handleId, "file:", fileIndex);

      // Build title - hide SXXEXX for movies
      const isMovie = media.media_type === "movie" || !!details.title;
      const playerTitle = isMovie 
        ? (details.title || details.name)
        : `${details.title || details.name} - S${pendingPlayRequest.season}E${pendingPlayRequest.episode}`;

      // Get saved progress for timestamp
      const progress = watchProgressStore.getProgress(details.id, media.media_type);
      let initialTimestamp = 0;
      
      // For TV shows, only use saved timestamp if we're playing the same episode
      if (!isMovie && progress && 
          progress.currentSeason === pendingPlayRequest.season && 
          progress.currentEpisode === pendingPlayRequest.episode) {
        initialTimestamp = progress.currentTimestamp || 0;
      } else if (isMovie && progress) {
        // For movies, always use saved timestamp
        initialTimestamp = progress.currentTimestamp || 0;
      }

      // Dispatch event to open video player
      window.dispatchEvent(
        new CustomEvent("openVideoPlayer", {
          detail: {
            src: null, // VideoPlayer will fetch this
            title: playerTitle,
            metadata: details, // Pass full details for watch history
            handleId: handleId,
            fileIndex: fileIndex,
            magnetLink: magnetLink,
            initialTimestamp: initialTimestamp,
            mediaId: details.id,
            mediaType: media.media_type,
            seasonNum: isMovie ? null : pendingPlayRequest.season,
            episodeNum: isMovie ? null : pendingPlayRequest.episode,
          },
        }),
      );
      return true;
    } catch (err) {
      console.error("Error preparing stream:", err);
      
      // If we failed to add the torrent (e.g. metadata fetch failed), clear the saved selection
      // This allows the user to pick a new torrent next time instead of getting stuck
      if (pendingPlayRequest && pendingPlayRequest.season && pendingPlayRequest.episode) {
        try {
          console.log("Clearing saved selection due to error");
          await invoke("remove_saved_selection", {
            show_id: details.id,
            season: pendingPlayRequest.season,
            episode: pendingPlayRequest.episode
          });
          torrentManagerRefresh++;
        } catch (removeErr) {
          console.warn("Failed to remove saved selection:", removeErr);
        }
      }
      
      showError("Failed to prepare stream. The saved torrent might be unavailable. Please try again to select a new source.");
      return false;
    }
  }

  function closeTorrentSelector() {
    showTorrentSelector = false;
    pendingPlayRequest = null;
  }

  const handleResearch = async (event) => {
    console.log("=== RESEARCH EVENT RECEIVED ===");
    console.log("Event detail:", event.detail);
    console.log("isSearching:", isSearching);
    console.log("pendingPlayRequest:", pendingPlayRequest);
    
    const { trackers, query: customQuery, useImdb } = event.detail;
    
    // Prevent concurrent searches
    if (isSearching) {
      console.log("Search already in progress, ignoring research request");
      return;
    }
    
    if (!pendingPlayRequest) {
      console.log("No pending play request, ignoring research");
      return;
    }
    
    console.log("=== STARTING RESEARCH ===");
    console.log("New trackers:", trackers);
    console.log("Custom query:", customQuery);
    console.log("Use IMDB:", useImdb);
    
    // Re-run the search with new tracker preference
    isSearching = true;
    searchResults = [];

    const showName = details.title || details.name;
    const isMovieCheck = media.media_type === "movie" || !!details.title;
    const { season: seasonNum, episode: episodeNum } = pendingPlayRequest;

    // Use custom query if provided, otherwise build default
    let searchQuery = customQuery;
    if (!searchQuery) {
      searchQuery = showName;
      if (isMovieCheck) {
        const year = (details.release_date || "").split("-")[0];
        if (year) {
          searchQuery = `${showName} ${year}`;
        }
      }
    }
    
    // Update the currentSearchQuery for display
    currentSearchQuery = searchQuery;

    const mediaType = isAnime() ? "anime" : (isMovieCheck ? "movie" : "tv");
    
    // If useImdb is true, re-fetch IMDB ID for EZTV
    let imdbIdToUse = currentImdbId;
    if (useImdb && !imdbIdToUse) {
      try {
        if (!isMovieCheck) {
          const externalIds = await getTVExternalIds(details.id);
          if (externalIds?.imdb_id) {
            imdbIdToUse = externalIds.imdb_id;
            currentImdbId = imdbIdToUse;
          }
        } else {
          const externalIds = await getMovieExternalIds(details.id);
          if (externalIds?.imdb_id) {
            imdbIdToUse = externalIds.imdb_id;
            currentImdbId = imdbIdToUse;
          }
        }
      } catch (err) {
        console.warn("Failed to re-fetch IMDB ID:", err);
      }
    }
    
    console.log("Invoking search_nyaa_filtered with:");
    console.log("- query:", searchQuery);
    console.log("- trackers:", trackers);
    console.log("- imdbId:", imdbIdToUse);
    
    try {
      searchResults = await invoke("search_nyaa_filtered", {
        query: searchQuery,
        season: isMovieCheck ? null : seasonNum,
        episode: isMovieCheck ? null : episodeNum,
        isMovie: isMovieCheck,
        mediaType: mediaType,
        trackerPreference: trackers && trackers.length > 0 ? trackers : null,
        imdbId: imdbIdToUse,
      });

      console.log(`=== RESEARCH COMPLETE ===`);
      console.log(`Found ${searchResults.length} results`);
    } catch (err) {
      console.error("Error during research:", err);
      searchResults = [];
    } finally {
      isSearching = false;
    }
  };

  function showManualFileSelector(torrent, info, handleId) {
    // Filter to video files only
    availableFiles = info.files.filter(f => {
      const ext = f.name.toLowerCase();
      return ext.endsWith('.mkv') || ext.endsWith('.mp4') || 
             ext.endsWith('.avi') || ext.endsWith('.mov') || 
             ext.endsWith('.webm') || ext.endsWith('.m4v');
    });
    
    if (availableFiles.length === 0) {
      showError("No video files found in this torrent.");
      return;
    }
    
    selectedTorrentForManual = torrent;
    manualHandleId = handleId;
    showFileSelector = true;
    showTorrentSelector = false;
  }

  async function handleFileSelectorConfirm(event) {
    const assignments = event.detail;
    
    if (!assignments || assignments.length === 0) {
      closeFileSelector();
      return;
    }

    try {
      for (const { file, season, episode } of assignments) {
        console.log(`saving S${season}E${episode} for file: ${file.name}`);
        await invoke("save_torrent_selection", {
          showId: details.id,
          season: season,
          episode: episode,
          magnetLink: selectedTorrentForManual.magnet_link,
          fileIndex: file.index,
        });
      }

      torrentManagerRefresh++;

      let playAssignment = assignments.find(
        a => a.season === pendingPlayRequest.season && a.episode === pendingPlayRequest.episode
      );
      
      if (!playAssignment) {
        playAssignment = assignments[0];
        pendingPlayRequest = { season: playAssignment.season, episode: playAssignment.episode };
      }

      startStream(selectedTorrentForManual.magnet_link, playAssignment.file.index, manualHandleId);
      closeFileSelector();
    } catch (err) {
      console.error("error saving file selections:", err);
      showError("Failed to save selections.");
    }
  }

  function closeFileSelector() {
    showFileSelector = false;
    availableFiles = [];
    selectedTorrentForManual = null;
    manualHandleId = null;
  }
</script>

<!-- svelte-ignore a11y-click-events-have-key-events -->
<!-- svelte-ignore a11y-no-static-element-interactions -->
<div class="detail-overlay" style="animation: fadeIn 0.3s ease;">
  <div
    class="detail-container"
    style="--backdrop-color: {backdropColor}; --prominent-color: {prominentColor}; --text-color: {textColor}"
  >
    <button
      class="close-btn btn-standard"
      on:click={() => dispatch("close")}
      title="Back"
    >
      <i class="ri-arrow-left-line"></i>
    </button>

    {#if loading}
      <div class="loading">Loading...</div>
    {:else if details}
      <div class="detail-backdrop">
        {#if details.backdrop_path}
          <img
            src={getImageUrl(details.backdrop_path, "w1280")}
            alt="Backdrop"
          />
        {/if}
      </div>

      <div class="detail-content">
        <div class="detail-header">
          <div class="detail-poster poster-large">
            {#if details.poster_path}
              <img
                src={getImageUrl(details.poster_path, "w500")}
                alt={details.title || details.name}
              />
            {/if}
          </div>

          <div class="detail-info-wrapper">
            <div class="detail-info">
              <h1 class="detail-title detail-title-large">
                {details.title || details.name}
              </h1>
              {#if details.tagline}
                <p class="detail-tagline detail-tagline-large">
                  "{details.tagline}"
                </p>
              {/if}

              <div class="detail-meta">
                {#if details.vote_average !== undefined && details.vote_average !== null}
                <div class="rating-box {getRatingClass(details.vote_average)}">
                  {details.vote_average.toFixed(1)}
                </div>
                {/if}
                {#if details.content_ratings?.results?.length || details.release_dates?.results?.length}
                  <span class="age-rating">
                    {#if details.content_ratings}
                      {details.content_ratings.results.find(
                        (r) => r.iso_3166_1 === "US",
                      )?.rating || "NR"}
                    {:else if details.release_dates}
                      {details.release_dates.results.find(
                        (r) => r.iso_3166_1 === "US",
                      )?.release_dates?.[0]?.certification || "NR"}
                    {/if}
                  </span>
                {/if}
                <span
                  >{formatDate(
                    details.release_date || details.first_air_date,
                  )}</span
                >
                {#if details.runtime}
                  <span>•</span>
                  <span>{formatRuntime(details.runtime)}</span>
                {/if}
                {#if details.number_of_seasons}
                  <span>•</span>
                  <span
                    >{details.number_of_seasons} Season{details.number_of_seasons >
                    1
                      ? "s"
                      : ""}</span
                  >
                {/if}
              </div>

              {#if details.genres && details.genres.length > 0}
                <div class="detail-genres">
                  {#each details.genres as genre}
                    <span class="genre-tag genre-tag-large">{genre.name}</span>
                  {/each}
                </div>
              {/if}

              <p class="detail-overview detail-overview-large">
                {details.overview}
              </p>

              <div class="detail-actions">
                <button
                  class="btn-standard primary btn-large play-btn-with-resume"
                  on:click={() => {
                    if (details.seasons && details.seasons.length > 0) {
                      // Use resume info if available, otherwise default to S1E1
                      if (resumeInfo && resumeInfo.season && resumeInfo.episode) {
                        handlePlay(resumeInfo.season, resumeInfo.episode);
                      } else {
                        handlePlay(1, 1);
                      }
                    } else {
                      // Movie
                      handlePlay(0, 0);
                    }
                  }}
                >
                  <i class="ri-play-fill"></i>
                  <div class="play-btn-content">
                    <span class="play-btn-main">{resumeInfo ? 'Resume' : 'Play'}</span>
                    {#if resumeInfo}
                      <span class="play-btn-resume-info">{resumeInfo.label}</span>
                    {/if}
                  </div>
                </button>
                <button class="btn-standard btn-large" on:click={toggleMyList}>
                  <i class={isInMyList ? "ri-check-line" : "ri-add-line"}></i>
                  {isInMyList ? "In My List" : "My List"}
                </button>
                <div class="more-menu-container">
                  <button
                    class="btn-standard btn-icon-only"
                    on:click={() => showMoreMenu = !showMoreMenu}
                    title="More Options"
                  >
                    <i class="ri-more-fill"></i>
                  </button>
                  {#if showMoreMenu}
                    <div class="more-menu">
                      <button class="menu-item" on:click={() => { showTorrentManager = true; showMoreMenu = false; }}>
                        <i class="ri-folder-download-line"></i>
                        <span>Manage Torrents</span>
                      </button>
                      <button class="menu-item" on:click={() => { reselectTorrent(); showMoreMenu = false; }}>
                        <i class="ri-refresh-line"></i>
                        <span>Reselect Torrent</span>
                      </button>
                    </div>
                  {/if}
                </div>
              </div>
            </div>
          </div>
        </div>

        <div class="detail-tabs">
          {#if details.seasons && details.seasons.length > 0}
            <button
              class="tab-btn"
              class:active={activeTab === "seasons"}
              on:click={() => (activeTab = "seasons")}
            >
              Seasons
            </button>
          {/if}
          <button
            class="tab-btn"
            class:active={activeTab === "cast"}
            on:click={() => (activeTab = "cast")}
          >
            Cast & Crew
          </button>
          <button
            class="tab-btn"
            class:active={activeTab === "details"}
            on:click={() => (activeTab = "details")}
          >
            Details
          </button>
        </div>

        <div class="tab-content">
          {#if activeTab === "seasons" && details.seasons && details.seasons.length > 0}
            <div class="seasons-header">
              <div class="episode-search">
                <i class="ri-search-line"></i>
                <input 
                  type="text" 
                  placeholder="Search episodes..." 
                  bind:value={episodeSearchQuery}
                />
                {#if episodeSearchQuery}
                  <button class="clear-search" on:click={() => episodeSearchQuery = ""}>
                    <i class="ri-close-line"></i>
                  </button>
                {/if}
              </div>
              <div class="view-toggle">
                <button
                  class="toggle-btn"
                  class:active={viewMode === "list"}
                  on:click={() => (viewMode = "list")}
                  title="List View"
                >
                  <i class="ri-list-check"></i>
                </button>
                <button
                  class="toggle-btn"
                  class:active={viewMode === "heatmap"}
                  on:click={() => (viewMode = "heatmap")}
                  title="Heatmap View"
                >
                  <i class="ri-grid-fill"></i>
                </button>
              </div>
            </div>

            {#if viewMode === "list"}
              <div class="seasons-accordion">
                {#each details.seasons.filter((s) => s.season_number > 0 && hasMatchingEpisodes(s.season_number, episodeSearchQuery)) as season}
                  <div
                    class="accordion-item"
                    class:expanded={selectedSeason === season.season_number || !!episodeSearchQuery}
                  >
                    <button
                      class="accordion-header"
                      on:click={() => toggleSeason(season.season_number)}
                    >
                      <div class="accordion-title">
                        <span class="season-name"
                          >Season {season.season_number}</span
                        >
                        <span class="episode-count"
                          >{season.episode_count} Episodes</span
                        >
                      </div>
                      <i class="ri-arrow-down-s-line accordion-icon"></i>
                    </button>

                    {#if selectedSeason === season.season_number || !!episodeSearchQuery}
                      <div class="accordion-content">
                        {#if allSeasonsData[season.season_number]}
                          <div class="episodes-list">
                            {#each allSeasonsData[season.season_number].episodes.filter(e => !episodeSearchQuery || e.name.toLowerCase().includes(episodeSearchQuery.toLowerCase()) || e.episode_number.toString().includes(episodeSearchQuery)) as episode}
                              {@const episodeKey = `${details.id}-${media.media_type}-S${season.season_number}-E${episode.episode_number}`}
                              {@const episodeProgress = $watchProgressStore[episodeKey]}
                              {@const percentage = episodeProgress && episodeProgress.duration ? (episodeProgress.currentTimestamp / episodeProgress.duration) * 100 : 0}
                              {@const isWatched = percentage > 85}
                              
                              <!-- svelte-ignore a11y-click-events-have-key-events -->
                              <!-- svelte-ignore a11y-no-static-element-interactions -->
                              <div
                                class="episode-list-item"
                                class:selected={selectedEpisode?.episode_number === episode.episode_number}
                                class:watched={isWatched}
                                on:click={() => (selectedEpisode = episode)}
                              >
                                <div class="episode-still">
                                  {#if episode.still_path}
                                    <img
                                      src={getImageUrl(episode.still_path, "w300")}
                                      alt={episode.name}
                                    />
                                  {:else}
                                    <div class="episode-placeholder">
                                      <i class="ri-film-line"></i>
                                    </div>
                                  {/if}
                                  
                                  {#if isWatched}
                                    <div class="watched-overlay">
                                      <i class="ri-check-line"></i>
                                    </div>
                                  {:else if percentage > 0}
                                    <div class="progress-bar-container">
                                      <div class="progress-bar" style="width: {percentage}%"></div>
                                    </div>
                                  {/if}
                                </div>
                                <div class="episode-details">
                                  <div class="episode-top">
                                    <span class="episode-num">E{episode.episode_number}</span>
                                    <span class="episode-name">{episode.name}</span>
                                  </div>
                                  <div class="episode-meta">
                                    {#if episode.vote_average}
                                      <span class="episode-rating {getRatingClass(episode.vote_average)}">
                                        {episode.vote_average.toFixed(1)}
                                      </span>
                                    {/if}
                                    <span class="episode-date">{formatDate(episode.air_date)}</span>
                                    {#if episode.runtime}
                                      <span>{episode.runtime}m</span>
                                    {/if}
                                  </div>
                                  <p class="episode-overview">{episode.overview}</p>
                                </div>
                              </div>
                            {/each}
                          </div>
                        {:else}
                          <div class="loading-season">Loading episodes...</div>
                        {/if}
                      </div>
                    {/if}
                  </div>
                {/each}
              </div>
            {:else if viewMode === "heatmap"}
              <div class="episodes-heatmap">
                <div class="heatmap-grid">
                  {#each details.seasons.filter((s) => s.season_number > 0) as season}
                    <div class="heatmap-row">
                      <div class="season-label">S{season.season_number}</div>
                      <div class="episodes-row">
                        {#if allSeasonsData[season.season_number]?.episodes}
                          {#each allSeasonsData[season.season_number].episodes as episode}
                            <!-- svelte-ignore a11y-click-events-have-key-events -->
                            <!-- svelte-ignore a11y-no-static-element-interactions -->
                            <div
                              class="heatmap-cell loaded {episode.vote_average
                                ? getRatingClass(episode.vote_average)
                                : 'rating-none'}"
                              on:click={() => {
                                selectedSeason = season.season_number;
                                selectedEpisode = episode;
                              }}
                              data-tooltip="{episode.name} - {episode.vote_average
                                ? episode.vote_average.toFixed(1)
                                : 'N/A'}"
                            >
                              {episode.vote_average
                                ? episode.vote_average.toFixed(1)
                                : "—"}
                            </div>
                          {/each}
                        {:else if season.episode_count}
                          {#each Array(season.episode_count) as _, episodeIndex}
                            <div class="heatmap-cell loading-cell">
                              <div class="loading-spinner"></div>
                            </div>
                          {/each}
                        {/if}
                      </div>
                    </div>
                  {/each}
                </div>
              </div>
            {/if}
          {/if}

          {#if activeTab === "cast" && credits}
            <div class="cast-crew-container">
              <div class="cast-section">
                <h3 class="section-subtitle">Cast</h3>
                <div class="cast-grid">
                  {#each credits.cast.slice(0, 20) as person}
                    <div class="cast-card">
                      {#if person.profile_path}
                        <img
                          src={getImageUrl(person.profile_path, "w185")}
                          alt={person.name}
                        />
                      {:else}
                        <div class="cast-placeholder">
                          <i class="ri-user-line"></i>
                        </div>
                      {/if}
                      <div class="cast-info">
                        <h4>{person.name}</h4>
                        <p>{person.character}</p>
                        {#if person.episode_count}
                          <span class="episode-count">{person.episode_count} episodes</span>
                        {/if}
                      </div>
                    </div>
                  {/each}
                </div>
              </div>

              {#if credits.crew && credits.crew.length > 0}
                <div class="crew-section">
                  <h3 class="section-subtitle">Crew</h3>
                  <div class="crew-grid">
                    {#each credits.crew.slice(0, 20) as person}
                      <div class="crew-card">
                        {#if person.profile_path}
                          <img
                            src={getImageUrl(person.profile_path, "w185")}
                            alt={person.name}
                          />
                        {:else}
                          <div class="crew-placeholder">
                            <i class="ri-user-line"></i>
                          </div>
                        {/if}
                        <div class="crew-info">
                          <h4>{person.name}</h4>
                          <p class="crew-job">{person.job}</p>
                          {#if person.episode_count}
                            <span class="episode-count">{person.episode_count} episodes</span>
                          {/if}
                        </div>
                      </div>
                    {/each}
                  </div>
                </div>
              {/if}
            </div>
          {/if}

          {#if activeTab === "details"}
            <div class="detail-grid">
              {#if details.status}
                <div class="detail-item">
                  <span class="label">Status</span>
                  <span class="value">{details.status}</span>
                </div>
              {/if}
              {#if details.budget}
                <div class="detail-item">
                  <span class="label">Budget</span>
                  <span class="value">{formatMoney(details.budget)}</span>
                </div>
              {/if}
              {#if details.revenue}
                <div class="detail-item">
                  <span class="label">Revenue</span>
                  <span class="value">{formatMoney(details.revenue)}</span>
                </div>
              {/if}
              {#if details.original_language}
                <div class="detail-item">
                  <span class="label">Language</span>
                  <span class="value"
                    >{details.original_language.toUpperCase()}</span
                  >
                </div>
              {/if}
              {#if details.vote_count}
                <div class="detail-item">
                  <span class="label">Votes</span>
                  <span class="value"
                    >{details.vote_count.toLocaleString()}</span
                  >
                </div>
              {/if}
              {#if details.production_companies && details.production_companies.length > 0}
                <div class="detail-item full-width">
                  <span class="label">Production</span>
                  <span class="value"
                    >{details.production_companies
                      .map((c) => c.name)
                      .join(", ")}</span
                  >
                </div>
              {/if}
              {#if details.networks && details.networks.length > 0}
                <div class="detail-item full-width">
                  <span class="label">Network</span>
                  <span class="value"
                    >{details.networks.map((n) => n.name).join(", ")}</span
                  >
                </div>
              {/if}
              {#if details.created_by && details.created_by.length > 0}
                <div class="detail-item full-width">
                  <span class="label">Created By</span>
                  <span class="value"
                    >{details.created_by.map((c) => c.name).join(", ")}</span
                  >
                </div>
              {/if}
            </div>
          {/if}
        </div>

        {#if recommendations.length > 0}
          <div class="recommendations-section">
            <h2 class="section-title">You May Also Like</h2>
            <div class="recommendations-grid">
              {#each recommendations as rec}
                <!-- svelte-ignore a11y-click-events-have-key-events -->
                <!-- svelte-ignore a11y-no-static-element-interactions -->
                <div
                  class="recommendation-card"
                  on:click={() =>
                    window.dispatchEvent(
                      new CustomEvent("openMediaDetail", { detail: rec }),
                    )}
                >
                  <div class="rec-poster">
                    {#if rec.poster_path}
                      <img
                        src={getImageUrl(rec.poster_path, "w342")}
                        alt={rec.title || rec.name}
                      />
                    {:else}
                      <div class="poster-placeholder">
                        <i class="ri-film-line"></i>
                      </div>
                    {/if}
                    {#if rec.vote_average}
                      <div class="rec-rating-badge {getRatingClass(rec.vote_average)}">
                        {rec.vote_average.toFixed(1)}
                      </div>
                    {/if}
                  </div>
                  <div class="recommendation-info">
                    <h4 class="rec-title">{rec.title || rec.name}</h4>
                    {#if rec.release_date || rec.first_air_date}
                      <span class="rec-year">
                        {(rec.release_date || rec.first_air_date).split('-')[0]}
                      </span>
                    {/if}
                  </div>
                </div>
              {/each}
            </div>
          </div>
        {/if}
      </div>
    {/if}
  </div>

  <div class="keyboard-shortcuts">
    <div class="shortcut-item">
      <kbd>ESC</kbd>
      <span>Close</span>
    </div>
  </div>
</div>

{#if selectedEpisode}
  <!-- svelte-ignore a11y-click-events-have-key-events -->
  <!-- svelte-ignore a11y-no-static-element-interactions -->
  <div class="episode-modal-overlay" on:click={() => (selectedEpisode = null)}>
    <!-- svelte-ignore a11y-click-events-have-key-events -->
    <!-- svelte-ignore a11y-no-static-element-interactions -->
    <div class="episode-modal" on:click={(e) => e.stopPropagation()}>
      <div class="episode-modal-header">
        <h3>
          Episode {selectedEpisode.episode_number}: {selectedEpisode.name}
        </h3>
        <div class="episode-modal-actions">
          <button
            class="btn-standard secondary"
            on:click={() => {
              handlePlay(selectedSeason, selectedEpisode.episode_number, true);
              selectedEpisode = null;
            }}
            title="Select a different torrent for this episode"
          >
            <i class="ri-refresh-line"></i> Change Torrent
          </button>
          <button
            class="btn-standard primary"
            on:click={() =>
              handlePlay(selectedSeason, selectedEpisode.episode_number)}
          >
            <i class="ri-play-fill"></i> Play
          </button>
          <button
            class="btn-standard close-modal-btn"
            on:click={() => (selectedEpisode = null)}
          >
            <i class="ri-close-line"></i>
          </button>
        </div>
      </div>

      <div class="episode-modal-content">
        {#if selectedEpisode.still_path}
          <div class="episode-modal-still">
            <img
              src={getImageUrl(selectedEpisode.still_path, "original")}
              alt={selectedEpisode.name}
            />
          </div>
        {/if}

        <div class="episode-modal-meta">
          {#if selectedEpisode.vote_average && selectedEpisode.vote_average > 0}
            <div class="episode-modal-stat">
              <span class="stat-label">Rating</span>
              <div class="stat-value">
                <span
                  class="rating-badge {getRatingClass(
                    selectedEpisode.vote_average,
                  )}"
                >
                  {selectedEpisode.vote_average.toFixed(1)}
                </span>
                {#if selectedEpisode.vote_count}
                  <span class="vote-count">({selectedEpisode.vote_count})</span>
                {/if}
              </div>
            </div>
          {/if}

          <div class="episode-modal-stat">
            <span class="stat-label">Air Date</span>
            <span class="stat-value"
              >{formatDate(selectedEpisode.air_date)}</span
            >
          </div>

          {#if selectedEpisode.runtime}
            <div class="episode-modal-stat">
              <span class="stat-label">Runtime</span>
              <span class="stat-value">{selectedEpisode.runtime} min</span>
            </div>
          {/if}
        </div>

        <div class="episode-modal-overview">
          <h4>Overview</h4>
          <p>{selectedEpisode.overview || "No overview available."}</p>
        </div>
      </div>
    </div>
  </div>
{/if}

{#if showErrorModal}
  <ErrorModal
    message={errorMessage}
    title={errorTitle}
    on:close={() => (showErrorModal = false)}
  />
{/if}
{#if showTorrentSelector}
  <TorrentSelector
    searchQuery={currentSearchQuery}
    originalSearchQuery={originalSearchQuery}
    results={searchResults}
    loading={isSearching}
    selectedTorrentName={selectedTorrentName}
    isAnime={isAnime()}
    hasImdbId={!!currentImdbId}
    isTVShow={media.media_type === 'tv'}
    on:select={onTorrentSelect}
    on:close={closeTorrentSelector}
    on:research={handleResearch}
  />
{/if}

{#if showFileSelector}
  <FileSelector
    files={availableFiles}
    showName={details.title || details.name}
    seasons={Object.keys(allSeasonsData).length > 0 ? Object.values(allSeasonsData) : (details.seasons || [])}
    on:confirm={handleFileSelectorConfirm}
    on:close={closeFileSelector}
  />
{/if}

{#if showTorrentManager}
  <TorrentManager
    {media}
    {details}
    {allSeasonsData}
    refreshTrigger={torrentManagerRefresh}
    on:selectTorrent={handleTorrentManagerSelect}
    on:close={handleTorrentManagerClose}
  />
{/if}
