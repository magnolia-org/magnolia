<script>
  import { onMount, onDestroy } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { formatTime } from "./utils/timeUtils.js";
  import { watchProgressStore } from "./stores/watchProgressStore.js";
  import { watchHistoryStore } from "./stores/watchHistoryStore.js";
  import { getSeasonDetails, getImageUrl } from "./tmdb.js";

  import { createEventDispatcher } from "svelte";

  export let src = "";
  export let metadata = null;
  export let title = "";
  export let handleId = null;
  export let fileIndex = null;
  export let magnetLink = null;
  export let initialTimestamp = 0;
  
  let videoMetadata = null;

  export let mediaId = null;
  export let mediaType = null;
  export let seasonNum = null;
  export let episodeNum = null;

  let loading = true;
  let loadingPhase = "initializing";
  let loadingStatus = {
    progress: 0,
    total: 0,
    speed: 0,
    peers: 0,
    status: "Initializing stream...",
    state: "checking",
    phaseProgress: 0,
  };
  let pollInterval;

  const dispatch = createEventDispatcher();

  const SEEK_TIME_SHORT = 5;
  const SEEK_TIME_LONG = 10;
  const VOLUME_STEP_SMALL = 0.1;
  const VOLUME_STEP_LARGE = 0.2;
  const CONTROLS_HIDE_TIMEOUT = 2000;
  const REFRESH_INTERVAL = 1000;

  // mpv container (no videoElement)
  let mpvContainer;
  let playing = false;
  let playbackRate = 1.0;
  let currentTime = 0;
  let duration = 0;
  let bufferedRanges = [];
  let torrentProgress = 0;
  let torrentBufferRanges = [];
  let volume = 1;
  let muted = false;
  let fullscreen = false;
  let showControls = true;
  let justSeeked = false;
  let showBufferingIndicator = false;
  let controlsTimeout;
  let isDragging = false;
  let progressBar;
  let videoContainer;

  let mpvUnlisteners = [];

  let showAudioMenu = false;
  let showSubtitleMenu = false;
  let showChaptersMenu = false;
  let showPlayerMenu = false;
  let showAudioSubmenu = false;
  let showSubtitleSubmenu = false;
  let showSubtitleSettings = false;
  let subtitleSettingsElement = null;
  let subtitleSettingsX = 0;
  let subtitleSettingsY = 0;
  
  const defaultSubtitleSettings = {
    fontSize: 24,
    backgroundOpacity: 0.5,
    color: '#ffffff',
    textShadow: false,
    textShadowColor: '#000000',
    windowMargin: 60
  };
  
  let subtitleSettings = { ...defaultSubtitleSettings };

  let selectedSubtitleLanguage = null;

  let playerMenuElement = null;
  let audioSubmenuElement = null;
  let subtitleSubmenuElement = null;
  let audioSubmenuX = 0;
  let audioSubmenuY = 0;
  let subtitleSubmenuX = 0;
  let subtitleSubmenuY = 0;
  let selectedAudioTrack = 0;
  let selectedSubtitleTrack = -1;
  let subtitleOffset = 0;
  let chapters = [];
  let externalSubtitles = [];
  let loadingExternalSubs = false;
  let loadingSubtitle = false;
  let loadingAudio = false;
  let lastSubtitleFetchKey = null;
  
  let playingInExternal = false;
  let showSkipPrompts = true;
  let clearCacheAfterWatch = false;
  let hideChapterMarkers = false;
  let cacheCleared = false;
  
  let torrentSessionId = null;
  let torrentFileId = null;
  let torrentHttpPort = null;
  let watchHistoryAdded = false;
  let saveTimeout;
  
  function getStableCacheId() {
    if (magnetLink) {
      const match = magnetLink.match(/xt=urn:btih:([a-fA-F0-9]+)/);
      if (match && match[1]) {
        return match[1].toLowerCase();
      }
    }
    return handleId ? String(handleId) : '0';
  }
  
  async function saveCacheMetadata(cacheId) {
    if (mediaId && mediaType && cacheId) {
      try {
        await invoke('save_cache_metadata', {
          hash: cacheId,
          tmdbId: Number(mediaId),
          mediaType: mediaType
        });
      } catch (error) {
        console.error('[cache metadata] failed to save mapping:', error);
      }
    }
  }

  // Convert #RRGGBB to mpv #RRGGBBAA (full opacity)
  function hexToMpvColor(hex) {
    return hex.replace(/^#/, '#') + 'FF';
  }

  async function applyAllSubtitleSettingsToMpv() {
    const { fontSize, color, backgroundOpacity, textShadow, textShadowColor, windowMargin } = subtitleSettings;
    const alpha = Math.round(backgroundOpacity * 255).toString(16).padStart(2, '0');
    const cmds = [
      ["sub-font-size", String(fontSize)],
      ["sub-color", hexToMpvColor(color)],
      ["sub-back-color", `#000000${alpha}`],
      ["sub-shadow-offset", textShadow ? "2" : "0"],
      ["sub-shadow-color", hexToMpvColor(textShadowColor)],
      ["sub-margin-y", String(windowMargin)],
    ];
    for (const [name, value] of cmds) {
      await invoke("mpv_set_option_string", { name, value }).catch(() => {});
    }
  }

  async function applyWyzieLangPreference(lang) {
    const match = externalSubtitles.find(s => s.language === lang);
    if (!match) return;
    const embeddedCount = videoMetadata?.subtitle_tracks?.length || 0;
    const idx = externalSubtitles.indexOf(match);
    await selectSubtitle(match, embeddedCount + idx);
  }

  async function loadTrackPreferences() {
    if (!magnetLink) return;
    try {
      const prefs = await invoke('get_track_preference', { magnetLink });

      // Determine language to restore: prefer per-torrent, fall back to last-used
      const langToRestore = prefs?.subtitle_language ||
        (!prefs?.subtitle_track_id ? localStorage.getItem('lastSubtitleLanguage') : null);

      if (prefs) {
        if (prefs.audio_track_id != null) {
          await invoke("mpv_run_command", { args: ["set", "aid", String(prefs.audio_track_id)] }).catch(() => {});
        }
        if (prefs.subtitle_track_id != null && prefs.subtitle_track_id > 0) {
          await invoke("mpv_run_command", { args: ["set", "sid", String(prefs.subtitle_track_id)] }).catch(() => {});
        }
        if (prefs.subtitle_offset != null) {
          subtitleOffset = prefs.subtitle_offset;
          await invoke("mpv_set_option_string", { name: "sub-delay", value: String(subtitleOffset) }).catch(() => {});
        }
      }

      if (langToRestore && externalSubtitles.length > 0) {
        await applyWyzieLangPreference(langToRestore);
      }
    } catch (error) {
      console.error('[track prefs] error loading preferences:', error);
    }
  }

  async function saveTrackPreferences() {
    if (!magnetLink) return;
    try {
      const audioTrackId = selectedAudioTrack > 0 ? selectedAudioTrack : null;
      const embeddedCount = videoMetadata?.subtitle_tracks?.length || 0;
      let subtitleTrackId = null;
      let subtitleLanguage = null;
      if (selectedSubtitleTrack >= 0) {
        if (selectedSubtitleTrack < embeddedCount) {
          // embedded — save the mpv track id for sid restoration
          subtitleTrackId = videoMetadata?.subtitle_tracks?.[selectedSubtitleTrack]?.id ?? null;
        } else {
          // wyzie — save language for cross-episode restoration
          subtitleLanguage = selectedSubtitleLanguage;
          if (subtitleLanguage) localStorage.setItem('lastSubtitleLanguage', subtitleLanguage);
        }
      }
      await invoke('save_track_preference', {
        magnetLink,
        audioTrackId,
        subtitleTrackId,
        subtitleLanguage,
        subtitleOffset: subtitleOffset !== 0 ? subtitleOffset : null
      });
    } catch (error) {
      console.error('[track prefs] error saving preferences:', error);
    }
  }

  let isSeeking = false;
  let seekPreviewTime = 0;
  let seekTimeout;
  let hoverTime = null;
  let hoverX = 0;

  // Visual indicators for shortcuts
  let showIndicator = false;
  let indicatorType = ""; // 'seek-forward', 'seek-backward', 'volume', 'play', 'pause', 'fullscreen', 'mute', 'unmute'
  let indicatorValue = "";
  let indicatorIcon = "";
  let indicatorTimeout;
  let indicatorAnimationKey = 0;
  let indicatorNudgeKey = 0; // Separate key for nudge animations
  let seekAccumulator = 0; // Accumulates seek time for stacking
  let lastSeekDirection = null; // 'forward' or 'backward'
  let lastIndicatorType = null; // Track last indicator for nudge detection
  let isExiting = false;
  let exitTimeout;
  let indicatorPosition = 'center';
  let progressTrackingInterval = null;
  let hasAddedToHistory = false;
  let hasSeekedToInitial = false;
  let skipSectionCheckInterval = null;

  const skipFilters = ['intro', 'op', 'opening', 'recap', 're-cap', 'eyecatch'];
  let currentSkipSection = null;
  let showSkipButton = false;
  let skipButtonTimeout = null;
  let skipTimeRemaining = 8;
  let skipTimerInterval = null;
  let skipTimerActive = false;
  let skipAnimationKey = 0;
  let showNextEpisodeButton = false;

  // Episode picker panel
  let showEpisodesPanel = false;
  let episodesPanelSeason = null;
  let episodesData = {};
  let loadingEpisodesPanel = false;

  // Episode name for player header
  let episodeName = null;

  // Seek/volume throttle to avoid IPC flooding when holding arrow keys
  let lastSeekTime = 0;
  let lastVolumeTime = 0;

  $: hasNextEpisode = (() => {
    if (!metadata || !metadata.seasons || seasonNum === null || episodeNum === null) return false;
    
    const currentSeason = metadata.seasons.find(s => s.season_number === seasonNum);
    if (!currentSeason) return false;
    
    if (episodeNum < currentSeason.episode_count) {
      return true;
    }
    
    // Check next season
    const nextSeason = metadata.seasons.find(s => s.season_number === seasonNum + 1);
    return !!nextSeason;
  })();

  $: seekChapter = chapters
    .filter((ch) => ch.start_time <= seekPreviewTime)
    .sort((a, b) => b.start_time - a.start_time)[0];

  // Sync mpv volume/mute when changed
  $: invoke("mpv_set_option_string", { name: "volume", value: String(Math.round(volume * 100)) }).catch(() => {});
  $: invoke("mpv_set_option_string", { name: "mute", value: muted ? "yes" : "no" }).catch(() => {});

  // Fetch external subtitles when media info is available
  $: {
    if (mediaId && mediaType) {
      const fetchKey = `${mediaId}-${mediaType}-${seasonNum}-${episodeNum}`;
      if (fetchKey !== lastSubtitleFetchKey) {
        loadExternalSubtitles(fetchKey);
      }
    }
  }

  async function fetchEpisodeName() {
    if (!mediaId || seasonNum == null || episodeNum == null) return;
    try {
      const seasonData = await getSeasonDetails(mediaId, seasonNum);
      const ep = seasonData?.episodes?.find(e => e.episode_number === episodeNum);
      episodeName = ep?.name || null;
    } catch (e) {
      // non-critical, ignore
    }
  }

  async function loadExternalSubtitles(fetchKey) {
    if (loadingExternalSubs) return;
    loadingExternalSubs = true;
    lastSubtitleFetchKey = fetchKey;
    
    try {
      const subs = await invoke('fetch_wyzie_subtitles', {
        tmdbId: String(mediaId),
        mediaType,
        season: seasonNum != null ? Number(seasonNum) : null,
        episode: episodeNum != null ? Number(episodeNum) : null,
      });
      // Sort subtitles alphabetically by language
      subs.sort((a, b) => {
        const langA = (a.language || "").toLowerCase();
        const langB = (b.language || "").toLowerCase();
        return langA.localeCompare(langB);
      });
      
      externalSubtitles = subs;
      console.log("loaded external subtitles:", externalSubtitles.length);
      
      // Try to apply subtitle preference if external subs were loaded
      if (subs.length > 0 && magnetLink) {
        setTimeout(() => loadTrackPreferences(), 100);
      }
    } catch (error) {
      console.error("failed to load external subtitles:", error);
      externalSubtitles = [];
    } finally {
      loadingExternalSubs = false;
    }
  }

  async function togglePlay() {
    await invoke("cycle_pause").catch(e => console.error("cycle_pause failed:", e));
  }

  async function setSpeed(rate) {
    playbackRate = rate;
    await invoke("mpv_set_option_string", { name: "speed", value: String(rate) }).catch(() => {});
  }

  async function startStreamProcess() {
    if (handleId === null || fileIndex === null) return;

    const numericHandle = Number(handleId);
    const numericFile = Number(fileIndex);

    loading = true;
    loadingPhase = "initializing";
    loadingStatus.status = "Preparing torrent stream...";
    loadingStatus.phaseProgress = 10;

    try {
      await invoke("prepare_stream", {
        handleId: numericHandle,
        fileIndex: numericFile,
      });
    } catch (error) {
      console.error("Failed to prepare stream:", error);
      loadingStatus.status = "Error preparing stream";
      loading = false;
      return;
    }

    const pollStatus = async () => {
      try {
        const status = await invoke("get_stream_status", {
          handleId: numericHandle,
          fileIndex: numericFile,
        });

        loadingStatus.progress = status.progress_bytes || 0;
        loadingStatus.total = status.total_bytes || 0;
        
        if (loadingStatus.total > 0) {
            torrentProgress = (loadingStatus.progress / loadingStatus.total) * 100;
        }

        loadingStatus.peers = status.peers || 0;
        loadingStatus.speed = status.download_speed
          ? status.download_speed * 125000
          : 0;

        if (status.status === "ready" && status.stream_info?.url && !src) {
          src = status.stream_info.url;
          if (pollInterval) {
            clearInterval(pollInterval);
            pollInterval = null;
          }
          // Hand off to mpv
          loadingStatus.status = "Starting player...";
          loadingStatus.phaseProgress = 90;
          await invoke("load_file", { path: src });
          // loading = false will be set by the file_loaded mpv event listener
        } else if (loadingPhase === "initializing") {
          loadingPhase = "buffering";
          loadingStatus.status = "Buffering stream...";
          loadingStatus.phaseProgress = 50;
        }
      } catch (error) {
        console.error("Failed to poll stream status:", error);
        loadingStatus.status = "Error loading stream";
      }
    };

    await pollStatus();
    if (!pollInterval) {
      pollInterval = setInterval(pollStatus, REFRESH_INTERVAL);
    }
  }

  function toggleMute() {
    muted = !muted;
    // Mute/unmute will be synced via reactive statements
  }

  function changeVolume(e) {
    volume = parseFloat(e.target.value);
    if (volume > 0) {
      muted = false;
      // HTML5 Audio syncs via reactive statements, no setMuted method
    }
    // Volume will be synced via reactive statements
  }

  async function toggleFullscreen() {
    const appWindow = getCurrentWindow();
    try {
      await appWindow.setFullscreen(!fullscreen);
      fullscreen = !fullscreen;
    } catch (err) {
      console.error("Fullscreen error:", err);
    }
  }

  function handleProgressHover(event) {
    if (!progressBar || !isFinite(duration)) return;
    const rect = progressBar.getBoundingClientRect();
    const ratio = Math.min(Math.max((event.clientX - rect.left) / rect.width, 0), 1);
    hoverTime = ratio * duration;
    hoverX = event.clientX - rect.left;
    if (isSeeking) {
      seekPreviewTime = hoverTime;
    }
  }

  function handleProgressLeave() {
    hoverTime = null;
    hoverX = 0;
  }

  function startDrag(event) {
    isSeeking = true;
    document.body.style.userSelect = "none";
    handleProgressHover(event);
  }

  function handleDrag(event) {
    if (!isSeeking) return;
    handleProgressHover(event);
  }

  function stopDrag(event) {
    if (!isSeeking) return;
    isSeeking = false;
    document.body.style.userSelect = "";
    handleProgressHover(event);
    if (hoverTime !== null && isFinite(duration)) {
      const newTime = Math.min(Math.max(hoverTime, 0), duration);
      currentTime = newTime;
      invoke("seek_video", { seconds: newTime }).catch(e => console.error("seek_video failed:", e));
    }
    justSeeked = true;
    setTimeout(() => {
      justSeeked = false;
    }, 500);
  }

  function handleMpvProgress(payload) {
    if (payload.time_pos !== undefined && payload.time_pos !== null) {
      currentTime = payload.time_pos;
    }
    if (payload.duration !== undefined && payload.duration !== null && payload.duration > 0) {
      duration = payload.duration;
    }
    playing = payload.is_playing;
    showBufferingIndicator = !!payload.is_buffering;

    // Check for cache clearing on completion
    if (duration > 0 && currentTime / duration > 0.9 && clearCacheAfterWatch && !cacheCleared && mediaId) {
      cacheCleared = true;
      invoke('clear_cache_item', { id: mediaId.toString() }).catch(e => console.error('Failed to clear cache', e));
    }

    checkSkipSections();

    // Track progress periodically (every 10 seconds)
    if (playing && !loading && currentTime > 0 && mediaId && mediaType) {
      if (!progressTrackingInterval) {
        progressTrackingInterval = setInterval(() => {
          if (playing && !loading && currentTime > 0 && mediaId && mediaType) {
            const progressData = {
              currentTimestamp: Math.floor(currentTime),
              duration: Math.floor(duration)
            };
            if (seasonNum !== null && episodeNum !== null) {
              progressData.currentSeason = seasonNum;
              progressData.currentEpisode = episodeNum;
            }
            watchProgressStore.updateProgress(mediaId, mediaType, progressData);
            if (!watchHistoryAdded && metadata) {
              const historyItem = {
                id: mediaId,
                media_type: mediaType,
                title: metadata.title || metadata.name || 'Unknown',
                poster_path: metadata.poster_path,
                backdrop_path: metadata.backdrop_path,
                release_date: metadata.release_date || metadata.first_air_date,
                vote_average: metadata.vote_average,
                ...progressData
              };
              watchHistoryStore.addItem(historyItem);
              watchHistoryAdded = true;
            }
          }
        }, 10000);
      }
    } else if (progressTrackingInterval && !playing) {
      clearInterval(progressTrackingInterval);
      progressTrackingInterval = null;
    }
  }

  function checkSkipSections() {
    if (!chapters || chapters.length === 0 || !duration) return;

    // Find current chapter (using start_time property)
    let currentChapter = null;
    for (let i = 0; i < chapters.length; i++) {
      const chapter = chapters[i];
      const nextChapter = chapters[i + 1];
      const chapterStart = chapter.start_time;
      const chapterEnd = nextChapter ? nextChapter.start_time : duration;
      
      if (currentTime >= chapterStart && currentTime < chapterEnd) {
        currentChapter = { ...chapter, end_time: chapterEnd };
        break;
      }
    }

    // Check for skippable section
    if (currentChapter && currentChapter.title) {
      const chapterTitle = currentChapter.title.toLowerCase();
      const isSkippable = skipFilters.some(filter => chapterTitle.includes(filter));

      if (isSkippable && currentSkipSection?.title !== currentChapter.title && showSkipPrompts) {
        // New skippable section detected
        console.log('Skip section detected:', currentChapter.title);
        currentSkipSection = currentChapter;
        showSkipButton = true;
        skipTimerActive = true;
        skipTimeRemaining = 8;
        skipAnimationKey++; // Force animation restart
        
        // Clear existing timers
        if (skipButtonTimeout) clearTimeout(skipButtonTimeout);
        if (skipTimerInterval) clearInterval(skipTimerInterval);
        
        // Start countdown timer
        skipTimerInterval = setInterval(() => {
          skipTimeRemaining--;
          if (skipTimeRemaining <= 0) {
            clearInterval(skipTimerInterval);
            skipTimerInterval = null;
            skipTimerActive = false;
          }
        }, 1000);
        
        // Auto-hide button after 8 seconds (only if controls aren't shown)
        skipButtonTimeout = setTimeout(() => {
          if (!showControls) {
            showSkipButton = false;
          }
          skipButtonTimeout = null;
        }, 8000);
      }
    } else if (currentSkipSection) {
      // Left the skip section - always hide button
      currentSkipSection = null;
      showSkipButton = false;
      skipTimerActive = false;
      if (skipButtonTimeout) {
        clearTimeout(skipButtonTimeout);
        skipButtonTimeout = null;
      }
      if (skipTimerInterval) {
        clearInterval(skipTimerInterval);
        skipTimerInterval = null;
      }
    }

    // Check for next episode button (ending section)
    if (duration > 0 && seasonNum !== null && episodeNum !== null) {
      let shouldShowNext = false;

      // Check if current chapter indicates ending
      if (currentChapter && currentChapter.title) {
        const title = currentChapter.title.toLowerCase();
        if (title.includes('ending') || (title.includes('credits') && !title.includes('opening')) || title === 'end') {
           shouldShowNext = true;
        }
      }

      // Fallback to existing logic: last chapter is short and at the end
      if (!shouldShowNext) {
        const endingThreshold = duration * 0.85; // Last 15% of video
        const lastChapter = chapters[chapters.length - 1];
        
        // Check if last chapter is in ending section AND duration is less than 15% of total
        if (lastChapter && lastChapter.start_time >= endingThreshold) {
          const lastChapterDuration = duration - lastChapter.start_time;
          const isShortEnding = lastChapterDuration <= (duration * 0.15);
          
          if (isShortEnding && currentTime >= lastChapter.start_time) {
            shouldShowNext = true;
          }
        }
      }
      
      if (shouldShowNext) {
        if (!showNextEpisodeButton) {
          showNextEpisodeButton = true;
        }
      } else if (showNextEpisodeButton) {
        showNextEpisodeButton = false;
      }
    }
  }

  function skipSection() {
    if (currentSkipSection) {
      invoke("seek_video", { seconds: currentSkipSection.end_time }).catch(e => console.error("seek_video failed:", e));
      showSkipButton = false;
      skipTimerActive = false;
      currentSkipSection = null;
      if (skipButtonTimeout) clearTimeout(skipButtonTimeout);
      if (skipTimerInterval) clearInterval(skipTimerInterval);
    }
  }

  async function goToNextEpisode() {
    if (seasonNum === null || episodeNum === null) return;

    // Update progress before switching
    if (mediaId && mediaType && currentTime > 0) {
      const progressData = {
        currentTimestamp: Math.floor(currentTime),
        duration: Math.floor(duration),
        currentSeason: seasonNum,
        currentEpisode: episodeNum
      };
      watchProgressStore.updateProgress(mediaId, mediaType, progressData);
    }

    const nextEpisode = episodeNum + 1;
    
    // Check if next episode torrent is tracked
    try {
      const trackedTorrent = await invoke('get_saved_selection', {
        showId: Number(mediaId),
        season: seasonNum,
        episode: nextEpisode
      });

      if (trackedTorrent && trackedTorrent.magnet_link) {
        console.log('Found saved torrent for next episode:', trackedTorrent);
        
        // Close current player before loading next episode
        dispatch('close');
        
        // Add the torrent (VideoPlayer will handle preparation)
        const handleResult = await invoke('add_torrent', {
          magnetOrUrl: trackedTorrent.magnet_link
        });
        
        // Format title with season and episode
        const showName = metadata?.name || metadata?.title || title;
        const episodeTitle = `${showName} - S${seasonNum}E${nextEpisode}`;
        
        // Dispatch event to update video player with new episode
        // VideoPlayer will handle stream preparation and show proper loading phases
        window.dispatchEvent(
          new CustomEvent('openVideoPlayer', {
            detail: {
              src: null, // Let VideoPlayer fetch the stream URL
              title: episodeTitle,
              metadata: metadata,
              handleId: handleResult,
              fileIndex: trackedTorrent.file_index,
              magnetLink: trackedTorrent.magnet_link,
              initialTimestamp: 0,
              mediaId: mediaId,
              mediaType: mediaType,
              seasonNum: seasonNum,
              episodeNum: nextEpisode,
            },
          }),
        );
      } else {
        // No saved torrent — open media detail so the user can pick a torrent for the next episode
        console.log('No saved torrent found, opening media detail for torrent selection');
        dispatch('close');
        window.dispatchEvent(new CustomEvent('openMediaDetail', {
          detail: {
            ...metadata,
            id: Number(mediaId),
            media_type: mediaType,
            autoPlay: true,
            resumeProgress: {
              currentSeason: seasonNum,
              currentEpisode: nextEpisode,
              currentTimestamp: 0
            }
          }
        }));
      }
    } catch (error) {
      console.error('Error navigating to next episode:', error);
      dispatch('close');
      window.dispatchEvent(new CustomEvent('openMediaDetail', {
        detail: {
          ...metadata,
          id: Number(mediaId),
          media_type: mediaType,
          autoPlay: true,
          resumeProgress: {
            currentSeason: seasonNum,
            currentEpisode: nextEpisode,
            currentTimestamp: 0
          }
        }
      }));
    }
  }

  async function syncFullscreenState() {
    const appWindow = getCurrentWindow();
    fullscreen = await appWindow.isFullscreen();
  }

  function close() {
    dispatch("close");
  }

  // formatTime moved to src/lib/utils/timeUtils.js

  function handleMouseMove() {
    if (!showControls) {
      showControls = true;
      window.dispatchEvent(new CustomEvent("videoControlsVisibility", { detail: { visible: true } }));
    }
    clearTimeout(controlsTimeout);
    controlsTimeout = setTimeout(() => {
      // Don't hide controls if any menu is open
      if (playing && !isSeeking && !showPlayerMenu && !showChaptersMenu && !showAudioSubmenu && !showSubtitleSubmenu) {
        showControls = false;
        window.dispatchEvent(new CustomEvent("videoControlsVisibility", { detail: { visible: false } }));
      }
    }, CONTROLS_HIDE_TIMEOUT);
  }


  function getCountryCode(languageCode) {
    if (!languageCode) return null;
    const code = languageCode.toLowerCase();
    const map = {
      'en': 'US', 'eng': 'US',
      'ja': 'JP', 'jpn': 'JP',
      'fr': 'FR', 'fra': 'FR', 'fre': 'FR',
      'de': 'DE', 'deu': 'DE', 'ger': 'DE',
      'es': 'ES', 'spa': 'ES',
      'it': 'IT', 'ita': 'IT',
      'pt': 'PT', 'por': 'PT',
      'ru': 'RU', 'rus': 'RU',
      'zh': 'CN', 'zho': 'CN', 'chi': 'CN',
      'ko': 'KR', 'kor': 'KR',
      'hi': 'IN', 'hin': 'IN',
      'ar': 'SA', 'ara': 'SA',
      'tr': 'TR', 'tur': 'TR',
      'pl': 'PL', 'pol': 'PL',
      'nl': 'NL', 'nld': 'NL', 'dut': 'NL',
      'sv': 'SE', 'swe': 'SE',
      'no': 'NO', 'nor': 'NO',
      'da': 'DK', 'dan': 'DK',
      'fi': 'FI', 'fin': 'FI',
      'vi': 'VN', 'vie': 'VN',
      'th': 'TH', 'tha': 'TH',
      'id': 'ID', 'ind': 'ID',
      'ms': 'MY', 'msa': 'MY', 'may': 'MY',
      'uk': 'UA', 'ukr': 'UA',
      'cs': 'CZ', 'ces': 'CZ', 'cze': 'CZ',
      'hu': 'HU', 'hun': 'HU',
      'ro': 'RO', 'ron': 'RO', 'rum': 'RO',
      'bg': 'BG', 'bul': 'BG',
      'el': 'GR', 'ell': 'GR', 'gre': 'GR',
      'he': 'IL', 'heb': 'IL',
      'fa': 'IR', 'fas': 'IR', 'per': 'IR',
      'ur': 'PK', 'urd': 'PK',
      'bn': 'BD', 'ben': 'BD',
      'ta': 'IN', 'tam': 'IN',
      'te': 'IN', 'tel': 'IN',
      'mr': 'IN', 'mar': 'IN',
      'gu': 'IN', 'guj': 'IN',
      'kn': 'IN', 'kan': 'IN',
      'ml': 'IN', 'mal': 'IN',
      'pa': 'IN', 'pan': 'IN',
      'sr': 'RS', 'srp': 'RS',
      'hr': 'HR', 'hrv': 'HR',
      'sl': 'SI', 'slv': 'SI',
      'sk': 'SK', 'slk': 'SK', 'slo': 'SK',
      'et': 'EE', 'est': 'EE',
      'lv': 'LV', 'lav': 'LV',
      'lt': 'LT', 'lit': 'LT',
      'ca': 'ES', 'cat': 'ES',
      'eu': 'ES', 'eus': 'ES', 'baq': 'ES',
      'gl': 'ES', 'glg': 'ES',
      'tl': 'PH', 'tgl': 'PH',
      'is': 'IS', 'isl': 'IS', 'ice': 'IS',
      'ga': 'IE', 'gle': 'IE',
      'cy': 'GB', 'cym': 'GB', 'wel': 'GB',
      'sq': 'AL', 'sqi': 'AL', 'alb': 'AL',
      'mk': 'MK', 'mkd': 'MK', 'mac': 'MK',
      'bs': 'BA', 'bos': 'BA',
      'az': 'AZ', 'aze': 'AZ',
      'kk': 'KZ', 'kaz': 'KZ',
      'uz': 'UZ', 'uzb': 'UZ',
      'hy': 'AM', 'hye': 'AM', 'arm': 'AM',
      'ka': 'GE', 'kat': 'GE', 'geo': 'GE',
      'be': 'BY', 'bel': 'BY',
      'mn': 'MN', 'mon': 'MN',
      'ne': 'NP', 'nep': 'NP',
      'si': 'LK', 'sin': 'LK',
      'km': 'KH', 'khm': 'KH',
      'lo': 'LA', 'lao': 'LA',
      'my': 'MM', 'mya': 'MM', 'bur': 'MM',
      'am': 'ET', 'amh': 'ET',
      'sw': 'TZ', 'swa': 'TZ',
      'af': 'ZA', 'afr': 'ZA',
      'zu': 'ZA', 'zul': 'ZA',
      'xh': 'ZA', 'xho': 'ZA',
      'st': 'ZA', 'sot': 'ZA',
      'tn': 'ZA', 'tsn': 'ZA',
      'ts': 'ZA', 'tso': 'ZA',
      'ss': 'ZA', 'ssw': 'ZA',
      've': 'ZA', 'ven': 'ZA',
      'nr': 'ZA', 'nbl': 'ZA',
      'ny': 'MW', 'nya': 'MW',
      'mg': 'MG', 'mlg': 'MG',
      'so': 'SO', 'som': 'SO',
      'ha': 'NG', 'hau': 'NG',
      'ig': 'NG', 'ibo': 'NG',
      'yo': 'NG', 'yor': 'NG',
      'rw': 'RW', 'kin': 'RW',
      'lg': 'UG', 'lug': 'UG',
      'ln': 'CD', 'lin': 'CD',
      'wo': 'SN', 'wol': 'SN',
      'ff': 'SN', 'ful': 'SN',
      'bm': 'ML', 'bam': 'ML',
      'dy': 'SN', 'dyu': 'SN',
      'ak': 'GH', 'aka': 'GH',
      'ee': 'GH', 'ewe': 'GH',
      'gaa': 'GH',
      'kri': 'SL',
      'men': 'SL',
      'tem': 'SL',
      'vai': 'LR',
      'kpe': 'LR',
      'man': 'GM',
      'sus': 'GN',
      'pulaar': 'SN',
      'soninke': 'ML',
      'zarma': 'NE',
      'hausa': 'NG',
      'kanuri': 'NG',
      'fulfulde': 'NG',
      'tamasheq': 'ML',
      'songhay': 'ML',
      'dogon': 'ML',
      'bambara': 'ML',
      'malinke': 'ML',
      'senufo': 'CI',
      'baoule': 'CI',
      'bete': 'CI',
      'dioula': 'CI',
      'yacouba': 'CI',
      'gueres': 'CI',
      'dida': 'CI',
      'abey': 'CI',
      'abidji': 'CI',
      'adoukrou': 'CI',
      'alladian': 'CI',
      'attie': 'CI',
      'ebrie': 'CI',
      'nzima': 'CI',
      'agni': 'CI',
      'abron': 'CI',
      'kulango': 'CI',
      'lobi': 'CI',
      'birifor': 'CI',
      'djimini': 'CI',
      'tagbana': 'CI',
      'jamala': 'CI',
      'nafana': 'CI',
      'koulango': 'CI',
      'ligbi': 'CI',
      'numu': 'CI',
      'humburi': 'ML',
      'koroboro': 'ML',
      'koyraboro': 'ML',
      'koyra': 'ML',
      'chiini': 'ML',
      'tasawaq': 'NE',
      'tedaga': 'TD',
      'dazaga': 'TD',
      'buduma': 'TD',
      'kotoko': 'CM',
      'mousgoum': 'CM',
      'massa': 'CM',
      'tupuri': 'CM',
      'mundang': 'CM',
      'gidar': 'CM',
      'fali': 'CM',
      'daba': 'CM',
      'guiziga': 'CM',
      'mofu': 'CM',
      'mafa': 'CM',
      'kapsiki': 'CM',
      'bana': 'CM',
      'zizilivakan': 'CM',
      'podoko': 'CM',
      'mandara': 'CM',
      'glavda': 'NG',
      'guduf': 'NG',
      'lamang': 'NG',
      'hide': 'NG',
      'vizik': 'NG',
      'vemgo': 'NG',
      'mabas': 'NG',
      'xedi': 'NG',
      'hdi': 'CM',
      'marga': 'NG',
      'kilba': 'NG',
      'bura': 'NG',
      'pabir': 'NG',
      'cibak': 'NG',
      'kamwe': 'NG',
      'margi': 'NG',
      'nggam': 'TD',
      'sar': 'TD',
      'mbay': 'TD',
      'ngambay': 'TD',
      'laka': 'TD',
      'kaba': 'TD',
      'gula': 'TD',
      'tumak': 'TD',
      'nancere': 'TD',
      'gabri': 'TD',
      'kwang': 'TD',
      'lele': 'TD',
      'kim': 'TD',
      'besme': 'TD',
      'mesme': 'TD',
      'masa': 'TD',
      'musey': 'TD',
      'marba': 'TD',
      'monogoy': 'TD',
      'kera': 'TD',
      'wina': 'CM',
      'giziga': 'CM',
      'north': 'CM',
      'south': 'CM',
      'baldemu': 'CM',
      'zulgwa': 'CM',
      'gemzek': 'CM',
      'minew': 'CM',
      'dugwor': 'CM',
      'mikiri': 'CM',
      'cuvok': 'CM',
      'merey': 'CM',
      'dugwur': 'CM',
      'mofu-gudur': 'CM',
      'mofu-north': 'CM',
      'mofu-south': 'CM',
      'tshang': 'CM',
      'gude': 'NG',
      'nzanyi': 'NG',
      'holma': 'NG',
      'bacama': 'NG',
      'bata': 'NG',
      'fali-mubi': 'NG',
      'fali-kiria': 'NG',
      'fali-jilbu': 'NG',
      'fali-gili': 'NG',
      'fali-bwahara': 'NG',
      'higi': 'NG',
      'bana': 'CM',
      'hya': 'CM',
      'psikye': 'CM',
      'kamwe': 'NG',
      'guduf-gava': 'NG',
      'glavda': 'NG',
      'cineni': 'NG',
      'dghwede': 'NG',
      'guduf': 'NG',
      'gava': 'NG',
      'cikide': 'NG',
      'chinene': 'NG',
      'nakatsa': 'NG',
      'gvoko': 'NG',
      'htan': 'NG',
      'tur': 'NG',
      'vemgo-mabas': 'NG',
      'lamang': 'NG',
      'hdi': 'CM',
      'mafa': 'CM',
      'matakam': 'CM',
      'mofu': 'CM',
      'cuvok': 'CM',
      'merey': 'CM',
      'dugwor': 'CM',
      'zulgwa': 'CM',
      'gemzek': 'CM',
      'minew': 'CM',
      'mikiri': 'CM',
      'dugwur': 'CM',
      'giziga': 'CM',
      'north': 'CM',
      'south': 'CM',
      'baldemu': 'CM',
      'wina': 'CM',
      'kera': 'TD',
      'kwang': 'TD',
      'lele': 'TD',
      'nancere': 'TD',
      'gabri': 'TD',
      'kim': 'TD',
      'besme': 'TD',
      'mesme': 'TD',
      'masa': 'TD',
      'musey': 'TD',
      'marba': 'TD',
      'monogoy': 'TD',
      'tupuri': 'CM',
      'mundang': 'CM',
      'gidar': 'CM',
      'fali': 'CM',
      'daba': 'CM',
      'guiziga': 'CM',
      'mofu': 'CM',
      'mafa': 'CM',
      'kapsiki': 'CM',
      'bana': 'CM',
      'zizilivakan': 'CM',
      'podoko': 'CM',
      'mandara': 'CM',
      'glavda': 'NG',
      'guduf': 'NG',
      'lamang': 'NG',
      'hide': 'NG',
      'vizik': 'NG',
      'vemgo': 'NG',
      'mabas': 'NG',
      'xedi': 'NG',
      'hdi': 'CM',
      'marga': 'NG',
      'kilba': 'NG',
      'bura': 'NG',
      'pabir': 'NG',
      'cibak': 'NG',
      'kamwe': 'NG',
      'margi': 'NG',
      'nggam': 'TD',
      'sar': 'TD',
      'mbay': 'TD',
      'ngambay': 'TD',
      'laka': 'TD',
      'kaba': 'TD',
      'gula': 'TD',
      'tumak': 'TD',
    };
    return map[code] || null;
  }

  async function selectAudioTrack(index) {
    const track = videoMetadata?.audio_tracks?.[index];
    if (!track) return;
    selectedAudioTrack = index;
    loadingAudio = true;
    try {
      await invoke("mpv_run_command", { args: ["set", "aid", String(track.id)] });
    } catch (e) {
      console.error("Failed to switch audio track:", e);
    } finally {
      loadingAudio = false;
      showAudioMenu = false;
    }
    saveTrackPreferences();
  }

  async function selectSubtitle(track, trackIndex) {
    selectedSubtitleTrack = trackIndex;
    loadingSubtitle = true;
    try {
      if (track.source === "wyzie") {
        // Load external subtitle via mpv sub-add
        await invoke("mpv_run_command", { args: ["sub-add", track.url, "select"] });
        selectedSubtitleLanguage = track.language;
      } else {
        // Embedded subtitle — use mpv track id
        const subTrack = videoMetadata?.subtitle_tracks?.[trackIndex];
        if (subTrack) {
          await invoke("mpv_run_command", { args: ["set", "sid", String(subTrack.id)] });
        }
        selectedSubtitleLanguage = null;
      }
      saveTrackPreferences();
    } catch (error) {
      console.error("Failed to load subtitle:", error);
    } finally {
      loadingSubtitle = false;
      showSubtitleMenu = false;
    }
  }

  async function disableSubtitles() {
    selectedSubtitleTrack = -1;
    selectedSubtitleLanguage = null;
    loadingSubtitle = false;
    await invoke("mpv_run_command", { args: ["set", "sid", "no"] }).catch(() => {});
    saveTrackPreferences();
    showSubtitleMenu = false;
  }

  async function jumpToChapter(startTime) {
    if (isFinite(startTime)) {
      const newTime = duration > 0 ? Math.min(startTime, duration) : startTime;
      currentTime = newTime;
      await invoke("seek_video", { seconds: newTime }).catch(e => console.error("seek_video failed:", e));
    }
    showChaptersMenu = false;
  }

  async function openInExternalPlayer() {
    try {
      const settings = await invoke('get_settings');
      const externalPlayer = settings.external_player || 'mpv';
      const installed = await invoke('check_external_player', { player: externalPlayer });
      if (!installed) {
        alert(`${externalPlayer.toUpperCase()} is not installed or not in PATH. Please install it to use external playback.`);
        return;
      }
      await invoke('open_in_external_player', {
        player: externalPlayer,
        streamUrl: src,
        title: title
      });
      playingInExternal = true;
      // Pause mpv when switching to external
      if (playing) {
        await invoke("cycle_pause").catch(() => {});
      }
      showPlayerMenu = false;
    } catch (error) {
      console.error('Failed to open in external player:', error);
      alert(`Failed to open external player: ${error}`);
    }
  }

  function restoreInternalPlayer() {
    playingInExternal = false;
    // Video will auto-resume if it was playing
  }

  function togglePlayerMenu() {
    showPlayerMenu = !showPlayerMenu;
    if (showPlayerMenu) {
      showChaptersMenu = false;
      showAudioSubmenu = false;
      showSubtitleSubmenu = false;
    } else {
      showAudioSubmenu = false;
      showSubtitleSubmenu = false;
    }
  }

  function toggleAudioSubmenu(event) {
    const wasOpen = showAudioSubmenu;
    showAudioSubmenu = !showAudioSubmenu;
    showSubtitleSubmenu = false;
    
    if (showAudioSubmenu) {
      const button = event.currentTarget;
      const buttonRect = button.getBoundingClientRect();
      
      // Store button position for initial render
      audioSubmenuX = buttonRect.left - 316;
      audioSubmenuY = buttonRect.top - 8;
      
      // Wait for submenu to render, then position it accurately
      setTimeout(() => {
        if (audioSubmenuElement) {
          const submenuWidth = audioSubmenuElement.offsetWidth;
          const submenuHeight = audioSubmenuElement.offsetHeight;
          
          // Position submenu to the left of the button with gap
          audioSubmenuX = buttonRect.left - submenuWidth - 16;
          
          // Align with button top, constrained to viewport
          const minY = 20;
          const maxY = window.innerHeight - submenuHeight - 20;
          audioSubmenuY = Math.max(minY, Math.min(buttonRect.top, maxY)) - 8;
          
          console.log('Audio submenu position:', { x: audioSubmenuX, y: audioSubmenuY, submenuWidth, buttonLeft: buttonRect.left });
        }
      }, 0);
    }
  }

  function toggleSubtitleSubmenu(event) {
    const wasOpen = showSubtitleSubmenu;
    showSubtitleSubmenu = !showSubtitleSubmenu;
    showAudioSubmenu = false;
    showSubtitleSettings = false;
    
    if (showSubtitleSubmenu) {
      const button = event.currentTarget;
      const buttonRect = button.getBoundingClientRect();
      
      // Store button position for initial render
      subtitleSubmenuX = buttonRect.left - 316;
      subtitleSubmenuY = buttonRect.top - 8;
      
      // Wait for submenu to render, then position it accurately
      setTimeout(() => {
        if (subtitleSubmenuElement) {
          const submenuWidth = subtitleSubmenuElement.offsetWidth;
          const submenuHeight = subtitleSubmenuElement.offsetHeight;
          
          // Position submenu to the left of the button with gap
          subtitleSubmenuX = buttonRect.left - submenuWidth - 16;
          
          // Align with button top, constrained to viewport
          const minY = 20;
          const maxY = window.innerHeight - submenuHeight - 20;
          subtitleSubmenuY = Math.max(minY, Math.min(buttonRect.top, maxY)) - 8;
          
          console.log('Subtitle submenu position:', { x: subtitleSubmenuX, y: subtitleSubmenuY, submenuWidth, buttonLeft: buttonRect.left });
        }
      }, 0);
    }
  }

  function toggleSubtitleSettings(event) {
    showSubtitleSettings = !showSubtitleSettings;
    
    if (showSubtitleSettings) {
      const button = event.currentTarget;
      const buttonRect = button.getBoundingClientRect();
      
      // Initial position
      subtitleSettingsX = buttonRect.left - 436; // Adjusted for wider menu (420px + padding)
      subtitleSettingsY = buttonRect.top - 8;
      
      setTimeout(() => {
        if (subtitleSettingsElement) {
          const width = subtitleSettingsElement.offsetWidth;
          const height = subtitleSettingsElement.offsetHeight;
          
          subtitleSettingsX = buttonRect.left - width - 16;
          
          const minY = 20;
          const maxY = window.innerHeight - height - 20;
          subtitleSettingsY = Math.max(minY, Math.min(buttonRect.top, maxY)) - 8;
        }
      }, 0);
    }
  }

  function handleGlobalClick(event) {
    if (showSubtitleSettings && subtitleSettingsElement && !subtitleSettingsElement.contains(event.target)) {
      showSubtitleSettings = false;
    }
    if (showEpisodesPanel && !event.target.closest('.episodes-panel') && !event.target.closest('.episodes-btn')) {
      showEpisodesPanel = false;
    }
  }

  function updateSubtitleSettings(key, value) {
    subtitleSettings = { ...subtitleSettings, [key]: value };
    localStorage.setItem('subtitleSettings', JSON.stringify(subtitleSettings));
    // Apply individual setting to mpv
    if (key === 'fontSize') {
      invoke("mpv_set_option_string", { name: "sub-font-size", value: String(value) }).catch(() => {});
    } else if (key === 'color') {
      invoke("mpv_set_option_string", { name: "sub-color", value: hexToMpvColor(value) }).catch(() => {});
    } else if (key === 'backgroundOpacity') {
      const alpha = Math.round(value * 255).toString(16).padStart(2, '0');
      invoke("mpv_set_option_string", { name: "sub-back-color", value: `#000000${alpha}` }).catch(() => {});
    } else if (key === 'textShadow') {
      invoke("mpv_set_option_string", { name: "sub-shadow-offset", value: value ? "2" : "0" }).catch(() => {});
    } else if (key === 'textShadowColor') {
      if (subtitleSettings.textShadow) {
        invoke("mpv_set_option_string", { name: "sub-shadow-color", value: hexToMpvColor(value) }).catch(() => {});
      }
    } else if (key === 'windowMargin') {
      invoke("mpv_set_option_string", { name: "sub-margin-y", value: String(value) }).catch(() => {});
    }
  }

  function updateSubtitleOffset(delta) {
    subtitleOffset = parseFloat((subtitleOffset + delta).toFixed(1));
    invoke("mpv_set_option_string", { name: "sub-delay", value: String(subtitleOffset) }).catch(() => {});
    if (saveTimeout) clearTimeout(saveTimeout);
    saveTimeout = setTimeout(() => saveTrackPreferences(), 1000);
  }

  function resetSubtitleOffset() {
    subtitleOffset = 0;
    invoke("mpv_set_option_string", { name: "sub-delay", value: "0" }).catch(() => {});
    saveTrackPreferences();
  }

  function resetSubtitleSetting(key) {
    updateSubtitleSettings(key, defaultSubtitleSettings[key]);
  }

  async function toggleHideChapters() {
    hideChapterMarkers = !hideChapterMarkers;
    try {
      const settings = await invoke('get_settings');
      settings.hide_chapter_markers = hideChapterMarkers;
      await invoke('save_settings', { settings });
    } catch (error) {
      console.error('failed to save hideChapterMarkers:', error);
    }
  }

  async function toggleSkipPrompts() {
    showSkipPrompts = !showSkipPrompts;
    try {
      const settings = await invoke('get_settings');
      settings.show_skip_prompts = showSkipPrompts;
      await invoke('save_settings', { settings });
      console.log('saved showSkipPrompts to backend:', showSkipPrompts);
    } catch (error) {
      console.error('failed to save showSkipPrompts:', error);
    }
  }

  async function goToNextEpisodeMenu() {
    if (seasonNum === null || episodeNum === null) return;

    const nextEpisode = episodeNum + 1;
    
    // Check if next episode torrent is tracked
    try {
      const trackedTorrent = await invoke('get_saved_selection', {
        showId: Number(mediaId),
        season: seasonNum,
        episode: nextEpisode
      });

      if (trackedTorrent && trackedTorrent.magnet_link) {
        console.log('Found saved torrent for next episode:', trackedTorrent);
        
        // Close current player before loading next episode
        dispatch('close');
        
        // Add the torrent (VideoPlayer will handle preparation)
        const handleResult = await invoke('add_torrent', {
          magnetOrUrl: trackedTorrent.magnet_link
        });
        
        // Format title with season and episode
        const showName = metadata?.name || metadata?.title || title;
        const episodeTitle = `${showName} - S${seasonNum}E${nextEpisode}`;
        
        // Dispatch event to update video player with new episode
        // VideoPlayer will handle stream preparation and show proper loading phases
        window.dispatchEvent(
          new CustomEvent('openVideoPlayer', {
            detail: {
              src: null, // Let VideoPlayer fetch the stream URL
              title: episodeTitle,
              metadata: metadata,
              handleId: handleResult,
              fileIndex: trackedTorrent.file_index,
              magnetLink: trackedTorrent.magnet_link,
              initialTimestamp: 0,
              mediaId: mediaId,
              mediaType: mediaType,
              seasonNum: seasonNum,
              episodeNum: nextEpisode,
            },
          }),
        );
      } else {
        // No saved torrent — open media detail so the user can pick a torrent for the next episode
        console.log('No saved torrent found, opening media detail for torrent selection');
        dispatch('close');
        window.dispatchEvent(new CustomEvent('openMediaDetail', {
          detail: {
            ...metadata,
            id: Number(mediaId),
            media_type: mediaType,
            autoPlay: true,
            resumeProgress: {
              currentSeason: seasonNum,
              currentEpisode: nextEpisode,
              currentTimestamp: 0
            }
          }
        }));
      }
    } catch (error) {
      console.error('Error navigating to next episode:', error);
      dispatch('close');
      window.dispatchEvent(new CustomEvent('openMediaDetail', {
        detail: {
          ...metadata,
          id: Number(mediaId),
          media_type: mediaType,
          autoPlay: true,
          resumeProgress: {
            currentSeason: seasonNum,
            currentEpisode: nextEpisode,
            currentTimestamp: 0
          }
        }
      }));
    }
  }

  function showShortcutIndicator(type, value, icon, seekAmount = 0, volumeDirection = null) {
    // Clear existing timeouts
    if (indicatorTimeout) clearTimeout(indicatorTimeout);
    if (exitTimeout) clearTimeout(exitTimeout);
    
    // Determine if this is a repeated action (nudge) or new action (appear)
    const isRepeatedAction = showIndicator && lastIndicatorType === type && !isExiting;
    
    // Handle seek stacking
    if (type === 'seek-forward' || type === 'seek-backward') {
      const direction = type === 'seek-forward' ? 'forward' : 'backward';
      
      // Reset accumulator if direction changed or indicator was hidden
      if (!showIndicator || lastSeekDirection !== direction || isExiting) {
        seekAccumulator = 0;
        lastSeekDirection = direction;
      }
      
      seekAccumulator += seekAmount;
      value = `${seekAccumulator >= 0 ? '+' : ''}${seekAccumulator}s`;
      
      // Set position for seek indicators
      indicatorPosition = type === 'seek-backward' ? 'left' : 'right';
    } else {
      // Reset seek accumulator for non-seek indicators
      seekAccumulator = 0;
      lastSeekDirection = null;
      indicatorPosition = 'center';
    }
    
    // For volume, use direction-specific type for proper animation
    if (type === 'volume' && volumeDirection) {
      indicatorType = volumeDirection === 'up' ? 'volume-up' : 'volume-down';
    } else {
      indicatorType = type;
    }
    
    indicatorValue = value;
    indicatorIcon = icon;
    lastIndicatorType = type;
    
    // Reset exiting state if we're showing a new indicator
    isExiting = false;
    showIndicator = true;
    
    if (isRepeatedAction) {
      // Just nudge, don't restart appear animation
      indicatorNudgeKey++;
    } else {
      // New action, restart full animation
      indicatorAnimationKey++;
    }
    
    // Start exit animation after delay
    indicatorTimeout = setTimeout(() => {
      isExiting = true;
      
      // Remove from DOM after exit animation completes
      exitTimeout = setTimeout(() => {
        showIndicator = false;
        isExiting = false;
        seekAccumulator = 0;
        lastSeekDirection = null;
        lastIndicatorType = null;
      }, 200); // Match CSS animation duration
    }, 600);
  }

  function handleKeyPress(event) {
    // Don't handle if user is typing in an input
    if (
      event.target.tagName === "INPUT" ||
      event.target.tagName === "TEXTAREA"
    ) {
      return;
    }

    switch (event.key.toLowerCase()) {
      case " ":
      case "k":
        event.preventDefault();
        togglePlay();
        showShortcutIndicator(
          playing ? "pause" : "play",
          playing ? "Pause" : "Play",
          playing ? "ri-pause-fill" : "ri-play-fill"
        );
        break;
      case "arrowleft":
        event.preventDefault();
        if (isFinite(currentTime)) {
          const newTime = Math.max(0, currentTime - SEEK_TIME_SHORT);
          currentTime = newTime;
          const _now1 = Date.now();
          if (!event.repeat || _now1 - lastSeekTime >= 80) {
            lastSeekTime = _now1;
            invoke("seek_video", { seconds: newTime }).catch(() => {});
          }
          showShortcutIndicator("seek-backward", "-5s", "ri-rewind-fill", -5);
        }
        break;
      case "arrowright":
        event.preventDefault();
        if (isFinite(currentTime) && isFinite(duration)) {
          const newTime = Math.min(duration, currentTime + SEEK_TIME_SHORT);
          currentTime = newTime;
          const _now2 = Date.now();
          if (!event.repeat || _now2 - lastSeekTime >= 80) {
            lastSeekTime = _now2;
            invoke("seek_video", { seconds: newTime }).catch(() => {});
          }
          showShortcutIndicator("seek-forward", "+5s", "ri-speed-fill", 5);
        }
        break;
      case "j":
        event.preventDefault();
        if (isFinite(currentTime)) {
          const newTime = Math.max(0, currentTime - SEEK_TIME_LONG);
          currentTime = newTime;
          const _now3 = Date.now();
          if (!event.repeat || _now3 - lastSeekTime >= 80) {
            lastSeekTime = _now3;
            invoke("seek_video", { seconds: newTime }).catch(() => {});
          }
          showShortcutIndicator("seek-backward", "-10s", "ri-rewind-fill", -10);
        }
        break;
      case "l":
        event.preventDefault();
        if (isFinite(currentTime) && isFinite(duration)) {
          const newTime = Math.min(duration, currentTime + SEEK_TIME_LONG);
          currentTime = newTime;
          const _now4 = Date.now();
          if (!event.repeat || _now4 - lastSeekTime >= 80) {
            lastSeekTime = _now4;
            invoke("seek_video", { seconds: newTime }).catch(() => {});
          }
          showShortcutIndicator("seek-forward", "+10s", "ri-speed-fill", 10);
        }
        break;
      case "arrowup":
        event.preventDefault(); {
          const _vt = Date.now();
          if (!event.repeat || _vt - lastVolumeTime >= 50) {
            lastVolumeTime = _vt;
            volume = Math.min(1, volume + VOLUME_STEP_SMALL);
            if (volume > 0 && muted) muted = false;
          }
          const iconUp = volume === 0 ? "ri-volume-mute-fill" : volume < 0.5 ? "ri-volume-down-fill" : "ri-volume-up-fill";
          showShortcutIndicator("volume", `${Math.round(volume * 100)}%`, iconUp, 0, 'up');
        }
        break;
      case "arrowdown":
        event.preventDefault(); {
          const _vt2 = Date.now();
          if (!event.repeat || _vt2 - lastVolumeTime >= 50) {
            lastVolumeTime = _vt2;
            volume = Math.max(0, volume - VOLUME_STEP_SMALL);
          }
          const iconDown = volume === 0 ? "ri-volume-mute-fill" : volume < 0.5 ? "ri-volume-down-fill" : "ri-volume-up-fill";
          showShortcutIndicator("volume", `${Math.round(volume * 100)}%`, iconDown, 0, 'down');
        }
        break;
      case "u":
        event.preventDefault(); {
          const _vt3 = Date.now();
          if (!event.repeat || _vt3 - lastVolumeTime >= 50) {
            lastVolumeTime = _vt3;
            volume = Math.max(0, volume - VOLUME_STEP_LARGE);
          }
          const iconU = volume === 0 ? "ri-volume-mute-fill" : volume < 0.5 ? "ri-volume-down-fill" : "ri-volume-up-fill";
          showShortcutIndicator("volume", `${Math.round(volume * 100)}%`, iconU, 0, 'down');
        }
        break;
      case "m":
        event.preventDefault();
        toggleMute();
        showShortcutIndicator(
          muted ? "mute" : "unmute",
          muted ? "Muted" : "Unmuted",
          muted ? "ri-volume-mute-fill" : "ri-volume-up-fill"
        );
        break;
      case "f":
        event.preventDefault();
        toggleFullscreen();
        showShortcutIndicator(
          fullscreen ? "exit-fullscreen" : "fullscreen",
          fullscreen ? "Exit Fullscreen" : "Fullscreen",
          fullscreen ? "ri-fullscreen-exit-fill" : "ri-fullscreen-fill"
        );
        break;
      case "a":
        event.preventDefault();
        updateSubtitleOffset(-0.1);
        showShortcutIndicator(
          "subtitle-offset",
          `${subtitleOffset > 0 ? '+' : ''}${subtitleOffset.toFixed(1)}s`,
          "ri-closed-captioning-line"
        );
        break;
      case "d":
        event.preventDefault();
        updateSubtitleOffset(0.1);
        showShortcutIndicator(
          "subtitle-offset",
          `${subtitleOffset > 0 ? '+' : ''}${subtitleOffset.toFixed(1)}s`,
          "ri-closed-captioning-line"
        );
        break;
      case "enter":
        event.preventDefault();
        if (showSkipButton && currentSkipSection) {
          skipSection();
        } else if (showNextEpisodeButton) {
          goToNextEpisode();
        }
        break;
    }
  }

  async function toggleEpisodesPanel() {
    showEpisodesPanel = !showEpisodesPanel;
    if (showEpisodesPanel) {
      // Close other menus
      showAudioMenu = false;
      showSubtitleMenu = false;
      showChaptersMenu = false;
      showPlayerMenu = false;
      // Default to current season
      const targetSeason = episodesPanelSeason ?? seasonNum ?? metadata?.seasons?.find(s => s.season_number > 0)?.season_number;
      if (targetSeason && !episodesData[targetSeason]) {
        await loadSeasonEpisodes(targetSeason);
      } else {
        episodesPanelSeason = targetSeason;
      }
    }
  }

  async function loadSeasonEpisodes(sNum) {
    if (!mediaId || !sNum) return;
    episodesPanelSeason = sNum;
    if (episodesData[sNum]) return;
    loadingEpisodesPanel = true;
    try {
      const data = await getSeasonDetails(mediaId, sNum);
      episodesData = { ...episodesData, [sNum]: data };
    } catch (e) {
      console.error('[episode panel] failed to load season:', e);
    } finally {
      loadingEpisodesPanel = false;
    }
  }

  async function playEpisodeFromPanel(targetSeason, targetEpisode) {
    showEpisodesPanel = false;

    // Update progress for current episode before switching
    if (mediaId && mediaType && currentTime > 0) {
      watchProgressStore.updateProgress(mediaId, mediaType, {
        currentTimestamp: Math.floor(currentTime),
        duration: Math.floor(duration),
        currentSeason: seasonNum,
        currentEpisode: episodeNum,
      });
    }

    try {
      const saved = await invoke('get_saved_selection', {
        showId: Number(mediaId),
        season: targetSeason,
        episode: targetEpisode,
      });

      if (saved && saved.magnet_link) {
        dispatch('close');
        const handleResult = await invoke('add_torrent', { magnetOrUrl: saved.magnet_link });
        const showName = metadata?.name || metadata?.title || title;
        window.dispatchEvent(new CustomEvent('openVideoPlayer', {
          detail: {
            src: null,
            title: `${showName} - S${targetSeason}E${targetEpisode}`,
            metadata,
            handleId: handleResult,
            fileIndex: saved.file_index,
            magnetLink: saved.magnet_link,
            initialTimestamp: 0,
            mediaId,
            mediaType,
            seasonNum: targetSeason,
            episodeNum: targetEpisode,
          },
        }));
      } else {
        // No saved torrent — open MediaDetail to select one
        dispatch('close');
        window.dispatchEvent(new CustomEvent('openMediaDetail', {
          detail: {
            id: Number(mediaId),
            media_type: mediaType,
            name: metadata?.name,
            title: metadata?.title,
            poster_path: metadata?.poster_path,
            autoPlay: true,
            resumeProgress: { currentSeason: targetSeason, currentEpisode: targetEpisode, currentTimestamp: 0 },
          },
        }));
      }
    } catch (err) {
      console.error('[episode panel] error switching episode:', err);
    }
  }

  onMount(async () => {
    console.log("VideoPlayer mounted");
    
    // Load settings from backend
    try {
      const settings = await invoke('get_settings');
      showSkipPrompts = settings.show_skip_prompts;
      clearCacheAfterWatch = settings.clear_cache_after_watch;
      hideChapterMarkers = settings.hide_chapter_markers ?? false;
    } catch (error) {
      console.error('Failed to load settings:', error);
    }
    
    // Sync fullscreen state when the native window changes (e.g. macOS green traffic light)
    const appWindow = getCurrentWindow();
    mpvUnlisteners.push(await appWindow.listen("tauri://resize", syncFullscreenState));
    window.addEventListener("mousemove", handleDrag);
    window.addEventListener("mouseup", stopDrag);
    window.addEventListener("keydown", handleKeyPress);
    window.addEventListener("click", handleGlobalClick);

    // Load global subtitle settings
    const savedSubtitleSettings = localStorage.getItem('subtitleSettings');
    if (savedSubtitleSettings) {
      try {
        subtitleSettings = { ...defaultSubtitleSettings, ...JSON.parse(savedSubtitleSettings) };
      } catch (e) {
        console.error('Failed to parse saved subtitle settings:', e);
      }
    }

    // Listen to mpv events
    mpvUnlisteners.push(await listen("mpv-progress-update", (event) => {
      handleMpvProgress(event.payload);
    }));

    mpvUnlisteners.push(await listen("mpv-tracks-update", (event) => {
      const tracks = event.payload.tracks || [];
      const subtitleTracks = tracks.filter(t => t.track_type === "sub");
      videoMetadata = {
        audio_tracks: tracks.filter(t => t.track_type === "audio"),
        subtitle_tracks: subtitleTracks,
      };
      // Sync selected subtitle track index from mpv's own selection state
      const selectedIdx = subtitleTracks.findIndex(t => t.selected);
      selectedSubtitleTrack = selectedIdx; // -1 when no sub track is selected
    }));

    mpvUnlisteners.push(await listen("mpv-chapters-update", (event) => {
      const raw = Array.isArray(event.payload) ? event.payload : [];
      chapters = raw.map((ch, i) => ({
        ...ch,
        start_time: ch.time ?? ch.start_time ?? 0,
        index: i,
      }));
    }));

    mpvUnlisteners.push(await listen("file_loaded", async () => {
      loading = false;
      playing = true;
      loadingPhase = "ready";
      if (initialTimestamp > 0 && !hasSeekedToInitial) {
        hasSeekedToInitial = true;
        await invoke("seek_video", { seconds: initialTimestamp }).catch(() => {});
      }
      setTimeout(async () => {
        await applyAllSubtitleSettingsToMpv();
        await loadTrackPreferences();
      }, 500);
    }));

    mpvUnlisteners.push(await listen("mpv-end-file", () => {
      playing = false;
    }));

    // Periodic skip section check
    skipSectionCheckInterval = setInterval(() => {
      if (currentSkipSection) {
        const stillInSection = currentTime >= currentSkipSection.start_time &&
                               currentTime < currentSkipSection.end_time;
        if (!stillInSection) {
          currentSkipSection = null;
          showSkipButton = false;
          skipTimerActive = false;
          if (skipButtonTimeout) { clearTimeout(skipButtonTimeout); skipButtonTimeout = null; }
          if (skipTimerInterval) { clearInterval(skipTimerInterval); skipTimerInterval = null; }
        }
      }
    }, 500);

    fetchEpisodeName();

    if (handleId !== null && fileIndex !== null) {
      startStreamProcess();
    } else {
      loading = false;
    }
  });

  onDestroy(async () => {
    clearInterval(pollInterval);
    if (progressTrackingInterval) clearInterval(progressTrackingInterval);
    if (skipButtonTimeout) clearTimeout(skipButtonTimeout);
    if (skipTimerInterval) clearInterval(skipTimerInterval);
    if (skipSectionCheckInterval) clearInterval(skipSectionCheckInterval);
    skipTimerActive = false;

    window.removeEventListener("mousemove", handleDrag);
    window.removeEventListener("mouseup", stopDrag);
    window.removeEventListener("keydown", handleKeyPress);
    window.removeEventListener("click", handleGlobalClick);
    clearTimeout(controlsTimeout);
    clearTimeout(indicatorTimeout);
    // Remove mpv event listeners
    for (const unlisten of mpvUnlisteners) unlisten();
    mpvUnlisteners = [];
    // Stop mpv playback so the native layer goes dark
    await invoke("mpv_run_command", { args: ["stop"] }).catch(() => {});
  });
</script>

<!-- svelte-ignore a11y-no-static-element-interactions -->
<div
  class="video-player"
  bind:this={videoContainer}
  on:mousemove={handleMouseMove}
  class:fullscreen
  class:hide-cursor={!showControls && playing}
>
  <!-- mpv renders into native view below this transparent WebView -->
  <div
    bind:this={mpvContainer}
    class="mpv-container"
    on:click={togglePlay}
  ></div>

  {#if loading}
    <div class="loading-overlay">
      {#if metadata?.backdrop_path}
        <img src={getImageUrl(metadata.backdrop_path, 'w1280')} alt="" class="loading-backdrop-img" aria-hidden="true" />
      {/if}
      <div class="loading-card">
        {#if metadata?.poster_path}
          <img src={getImageUrl(metadata.poster_path, 'w185')} alt="" class="loading-poster" />
        {/if}
        <div class="loading-info">
          <div class="loading-title">{metadata?.title || metadata?.name || title}</div>
          {#if seasonNum != null && episodeNum != null}
            <div class="loading-episode-label">Season {seasonNum} &bull; Episode {episodeNum}</div>
          {/if}
          <div class="loading-progress-row">
            <div class="loading-bar"></div>
          </div>
          <div class="loading-status-text">{loadingStatus.status}</div>
          <button class="cancel-loading-btn" on:click={close}>Cancel</button>
        </div>
      </div>
    </div>
  {/if}

  <!-- Buffering indicator -->
  {#if showBufferingIndicator}
    <div class="buffering-indicator">
      <div class="buffering-spinner">
        <i class="ri-loader-4-line"></i>
      </div>
    </div>
  {/if}

  <div class="player-header" class:visible={showControls}>
    <button class="back-btn" on:click={close}>
      <i class="ri-arrow-left-line"></i>
    </button>
    <div class="player-title">
      <span class="player-title-main">{metadata?.title || metadata?.name || title}</span>
      {#if episodeName}
        <span class="player-title-sub">S{String(seasonNum).padStart(2,'0')}E{String(episodeNum).padStart(2,'0')} &bull; {episodeName}</span>
      {:else if seasonNum != null && episodeNum != null}
        <span class="player-title-sub">S{String(seasonNum).padStart(2,'0')}E{String(episodeNum).padStart(2,'0')}</span>
      {/if}
    </div>
  </div>

  <!-- SubtitlesOctopus automatically creates and manages its own canvas as a sibling of the video element -->

  <!-- Keyboard shortcut indicator -->
  {#if showIndicator}
    <div class="shortcut-indicator {indicatorType} {indicatorPosition}" class:exiting={isExiting}>
      {#key indicatorAnimationKey}
        <div class="indicator-content">
          {#key indicatorNudgeKey}
            <div class="indicator-icon">
              <i class="{indicatorIcon}"></i>
            </div>
          {/key}
          <div class="indicator-value">{indicatorValue}</div>
        </div>
      {/key}
    </div>
  {/if}

  <!-- Skip Section Button -->
  {#if showSkipPrompts && chapters && chapters.length > 0 && currentSkipSection && (skipTimerActive || showControls)}
    <button class="skip-button" on:click={skipSection}>
      <span class="skip-text">Skip {currentSkipSection.title}</span>
      <kbd class="skip-kbd">
        <i class="ri-corner-down-left-line"></i>
      </kbd>
      {#if skipTimerActive}
        {#key skipAnimationKey}
          <div class="skip-timer">
            <svg class="skip-timer-spinner" viewBox="0 0 20 20">
              <circle
                class="skip-timer-circle"
                cx="10"
                cy="10"
                r="8"
              />
            </svg>
          </div>
        {/key}
      {/if}
    </button>
  {/if}

  <!-- Next Episode Button -->
  {#if chapters && chapters.length > 0 && showNextEpisodeButton && seasonNum !== null && episodeNum !== null && hasNextEpisode}
    <button class="skip-button next-episode" on:click={goToNextEpisode}>
      <span class="skip-text">Next Episode</span>
      <kbd class="skip-kbd">
        <i class="ri-corner-down-left-line"></i>
      </kbd>
    </button>
  {/if}

  <div class="controls" class:visible={showControls}>
    <!-- svelte-ignore a11y-no-static-element-interactions -->
    <div
      class="progress-bar"
      bind:this={progressBar}
      on:mousedown={startDrag}
      on:mousemove={handleProgressHover}
      on:mouseleave={handleProgressLeave}
    >
      <div
        class="progress-torrent"
        style="width: {torrentProgress}%"
      ></div>
      <!-- Segmented buffer hidden due to visual bugs
      {#each bufferedRanges as range}
        <div
          class="progress-buffered"
          style="left: {range.start}%; width: {range.width}%"
        ></div>
      {/each}
      -->
      <div
        class="progress-filled"
        style="width: {((isSeeking ? seekPreviewTime : currentTime) /
          duration) *
          100}%; transition: {(isSeeking || justSeeked) ? 'none' : 'width 0.1s linear'}"
      >
        <div class="progress-handle"></div>
      </div>

            <!-- Chapter markers -->
      {#if !hideChapterMarkers && chapters && chapters.length > 0}
        {#each chapters as chapter}
          {#if chapter.start_time > 1}
            <div
              class="chapter-marker"
              style="left: {(chapter.start_time / duration) * 100}%"
              title="{formatTime(chapter.start_time)} - {chapter.title ||
                `Chapter ${chapter.index + 1}`}"
            ></div>
          {/if}
        {/each}
      {/if}

      <!-- Hover preview tooltip -->
      {#if hoverTime !== null && !isSeeking}
        {@const hoverChapter = chapters
          .filter((ch) => ch.start_time <= hoverTime)
          .sort((a, b) => b.start_time - a.start_time)[0]}
        <div class="time-tooltip" style="left: {hoverX}px">
          <div class="tooltip-time">{formatTime(hoverTime)}</div>
          {#if hoverChapter}
            <div class="tooltip-chapter">
              {hoverChapter.title || `Chapter ${hoverChapter.index + 1}`}
            </div>
          {/if}
        </div>
      {/if}

      <!-- Seeking preview tooltip -->
      {#if isSeeking}
        {@const seekChapterMatch = chapters
          .filter((ch) => ch.start_time <= seekPreviewTime)
          .sort((a, b) => b.start_time - a.start_time)[0]}
        <div
          class="time-tooltip"
          style="left: {(seekPreviewTime / duration) *
            progressBar?.getBoundingClientRect().width || 0}px"
        >
          <div class="tooltip-time">{formatTime(seekPreviewTime)}</div>
          {#if seekChapterMatch}
            <div class="tooltip-chapter">
              {seekChapterMatch.title ||
                `Chapter ${seekChapterMatch.index + 1}`}
            </div>
          {/if}
        </div>
      {/if}
    </div>

    <div class="control-buttons">
      <button on:click={togglePlay} class="play-btn">
        <i class={playing ? "ri-pause-fill" : "ri-play-fill"}></i>
      </button>

      <span class="time"
        >{formatTime(currentTime)} / {formatTime(duration)}</span
      >

      <div class="spacer"></div>

      <div class="volume-control">
        <button on:click={toggleMute} class="volume-btn control-btn">
          {#if muted || volume === 0}
            <i class="ri-volume-mute-fill"></i>
          {:else if volume < 0.5}
            <i class="ri-volume-down-fill"></i>
          {:else}
            <i class="ri-volume-up-fill"></i>
          {/if}
        </button>
        <div class="volume-slider-wrapper">
          <div class="volume-slider-track">
            <div
              class="volume-slider-fill"
              style="height: {volume * 100}%"
            ></div>
            <div
              class="volume-slider-thumb"
              style="bottom: {volume * 100}%"
            ></div>
          </div>
          <input
            type="range"
            min="0"
            max="1"
            step="0.01"
            value={volume}
            on:input={changeVolume}
            orient="vertical"
            class="volume-slider-input"
          />
        </div>
      </div>

      {#if metadata && metadata.seasons && metadata.seasons.length > 0}
        <button
          class="episodes-btn control-btn"
          on:click|stopPropagation={toggleEpisodesPanel}
          title="Episodes"
        >
          <i class="ri-film-line"></i>
        </button>
      {/if}

      {#if chapters && chapters.length > 0}
        <div class="player-track-menu-container">
          <button
            on:click={() => {
              showChaptersMenu = !showChaptersMenu;
              if (showChaptersMenu) {
                showAudioMenu = false;
                showSubtitleMenu = false;
                showPlayerMenu = false;
              }
            }}
            class="chapters-btn control-btn"
          >
            <i class="ri-list-check"></i>
          </button>
          {#if showChaptersMenu}
            <div class="player-track-dropdown chapters-menu">
              {#each chapters as chapter}
                <button
                  class="player-track-option"
                  on:click={() => jumpToChapter(chapter.start_time)}
                >
                  <span class="chapter-time"
                    >{formatTime(chapter.start_time)}</span
                  >
                  <span class="chapter-title"
                    >{chapter.title || `Chapter ${chapter.index + 1}`}</span
                  >
                </button>
              {/each}
            </div>
          {/if}
        </div>
      {/if}

      <div class="player-track-menu-container">
        <button
          on:click={togglePlayerMenu}
          class="menu-btn control-btn"
        >
          <i class="ri-settings-3-line"></i>
        </button>
        {#if showPlayerMenu}
          <div class="player-track-dropdown player-menu" bind:this={playerMenuElement}>
            <!-- Audio Submenu -->
            {#if videoMetadata?.audio_tracks && videoMetadata.audio_tracks.length > 0}
              <div class="submenu-container">
                <button
                  class="player-track-option menu-item submenu-trigger"
                  on:click={toggleAudioSubmenu}
                >
                  <span class="player-track-info">
                    <i class="ri-music-2-line"></i> Audio Track
                  </span>
                  <i class="ri-arrow-right-s-line"></i>
                </button>
              </div>
            {/if}

            <!-- Subtitle Submenu -->
            {#if (videoMetadata?.subtitle_tracks && videoMetadata.subtitle_tracks.length > 0) || externalSubtitles.length > 0}
              <div class="submenu-container">
                <button
                  class="player-track-option menu-item submenu-trigger"
                  on:click={toggleSubtitleSubmenu}
                >
                  <span class="player-track-info">
                    <i class="ri-closed-captioning-line"></i> Subtitles
                  </span>
                  <i class="ri-arrow-right-s-line"></i>
                </button>
              </div>
            {/if}

            <!-- Speed -->
            <div class="player-track-option menu-item speed-item">
              <div class="speed-item-header">
                <span class="player-track-info">
                  <i class="ri-speed-line"></i> Speed
                </span>
                <span class="menu-speed-value">{playbackRate}x</span>
              </div>
              <input
                type="range"
                class="speed-slider"
                min="0.25"
                max="3"
                step="0.25"
                value={playbackRate}
                on:input={(e) => setSpeed(parseFloat(e.target.value))}
              />
            </div>

            <div class="menu-divider"></div>

            <!-- Chapter options -->
            {#if chapters && chapters.length > 0}
              <button class="player-track-option menu-item" on:click={toggleHideChapters}>
                <span class="player-track-info">
                  <i class="ri-bookmark-line"></i> Chapter markers
                </span>
                <span class="menu-toggle-indicator" class:on={!hideChapterMarkers}></span>
              </button>
            {/if}

            <!-- Actions -->
            <button
              class="player-track-option menu-item"
              on:click={openInExternalPlayer}
            >
              <span class="player-track-info">
                <i class="ri-external-link-line"></i> Open in external player
              </span>
            </button>
          </div>
        {/if}

        <!-- Audio Submenu (floating) -->
        {#if showAudioSubmenu && videoMetadata?.audio_tracks}
          <div class="submenu" style="left: {audioSubmenuX}px; top: {audioSubmenuY}px;" bind:this={audioSubmenuElement}>
            {#each videoMetadata.audio_tracks as track, i}
              <button
                class="player-track-option menu-item"
                class:active={selectedAudioTrack === i}
                disabled={loadingAudio && selectedAudioTrack !== i}
                on:click={() => selectAudioTrack(i)}
              >
                <span class="player-track-info">
                  {#if track.lang}
                    {@const countryCode = getCountryCode(track.lang)}
                    {#if countryCode}
                      <img 
                        src="https://flagcdn.com/w40/{countryCode.toLowerCase()}.png" 
                        alt={track.lang}
                        class="track-flag"
                      />
                    {/if}
                    <span class="player-track-lang">{track.lang.toUpperCase()}</span>
                  {:else}
                    Track {i + 1}
                  {/if}
                  {#if track.title}
                    <span class="player-track-detail">({track.title})</span>
                  {/if}
                </span>
                {#if loadingAudio && selectedAudioTrack === i}
                  <div class="loading-spinner-small"></div>
                {:else if track.codec}
                  <span class="player-track-badge">{track.codec}</span>
                {/if}
              </button>
            {/each}
          </div>
        {/if}

        <!-- Subtitle Submenu (floating) -->
        {#if showSubtitleSubmenu && ((videoMetadata?.subtitle_tracks && videoMetadata.subtitle_tracks.length > 0) || externalSubtitles.length > 0)}
          <div class="submenu" style="left: {subtitleSubmenuX}px; top: {subtitleSubmenuY}px;" bind:this={subtitleSubmenuElement}>
            <button
              class="player-track-option menu-item submenu-trigger"
              on:click|stopPropagation={toggleSubtitleSettings}
            >
              <span class="player-track-info">
                <i class="ri-settings-4-line"></i> Customize
              </span>
              <i class="ri-arrow-right-s-line"></i>
            </button>
            <div class="menu-divider"></div>

            <button
              class="player-track-option menu-item"
              class:active={selectedSubtitleTrack === -1}
              on:click={disableSubtitles}
            >
              <span class="player-track-info">Off</span>
            </button>
            {#if videoMetadata?.subtitle_tracks && videoMetadata.subtitle_tracks.length > 0}
              {#each videoMetadata.subtitle_tracks as track, i}
                {#if (track.codec || '').toLowerCase() !== 'hdmv_pgs_subtitle'}
                  <button
                    class="player-track-option menu-item"
                    class:active={selectedSubtitleTrack === i}
                    on:click={() => selectSubtitle(track, i)}
                    disabled={loadingSubtitle && selectedSubtitleTrack !== i}
                  >
                    <span class="player-track-info">
                      {#if track.lang}
                        {@const countryCode = getCountryCode(track.lang)}
                        {#if countryCode}
                          <img 
                            src="https://flagcdn.com/w40/{countryCode.toLowerCase()}.png" 
                            alt={track.lang}
                            class="track-flag"
                          />
                        {/if}
                        <span class="player-track-lang">{track.lang.toUpperCase()}</span>
                      {:else}
                        <span class="player-track-lang">Subtitle {i + 1}</span>
                      {/if}
                      {#if track.title}
                        <span class="player-track-detail">{track.title}</span>
                      {/if}
                    </span>
                    <span class="player-track-badge">{track.codec || 'MKV'}</span>
                    {#if loadingSubtitle && selectedSubtitleTrack === i}
                      <span class="loading-spinner-small"></span>
                    {/if}
                  </button>
                {/if}
              {/each}
            {/if}
            {#if externalSubtitles.length > 0}
              {@const embeddedCount = videoMetadata?.subtitle_tracks?.length || 0}
              {#each externalSubtitles as track, i}
                {@const trackIndex = embeddedCount + i}
                <button
                  class="player-track-option menu-item"
                  class:active={selectedSubtitleTrack === trackIndex}
                  on:click={() => selectSubtitle(track, trackIndex)}
                  disabled={loadingSubtitle && selectedSubtitleTrack !== trackIndex}
                >
                  <span class="player-track-info">
                    {#if track.language}
                      {@const countryCode = getCountryCode(track.language)}
                      {#if countryCode}
                        <img 
                          src="https://flagsapi.com/{countryCode}/flat/64.png" 
                          alt={track.language}
                          class="track-flag"
                        />
                      {/if}
                      <span class="player-track-lang">{track.language.toUpperCase()}</span>
                    {:else}
                      <span class="player-track-lang">External {i + 1}</span>
                    {/if}
                  </span>
                  {#if track.source === 'wyzie'}
                    <span class="player-track-badge wyzie-badge">WYZIE</span>
                  {:else}
                    <span class="player-track-badge">SRT</span>
                  {/if}
                </button>
              {/each}
            {/if}
          </div>
        {/if}

        <!-- Subtitle Settings Submenu -->
        {#if showSubtitleSettings}
          <div class="submenu settings-submenu" style="left: {subtitleSettingsX}px; top: {subtitleSettingsY}px;" bind:this={subtitleSettingsElement}>
            <div class="settings-preview-container">
              <div class="settings-preview-text" style="
                font-size: {subtitleSettings.fontSize}px;
                background: rgba(0, 0, 0, {subtitleSettings.backgroundOpacity});
                color: {subtitleSettings.color};
                text-shadow: {subtitleSettings.textShadow ? `2px 2px 2px ${subtitleSettings.textShadowColor}` : 'none'};
              ">
                Preview Text
              </div>
            </div>
            
            <div class="settings-group">
              <label>Size</label>
              <div class="settings-controls-wrapper">
                <div class="settings-controls">
                  <button class="settings-btn" on:click|stopPropagation={() => updateSubtitleSettings('fontSize', Math.max(12, subtitleSettings.fontSize - 2))}>-</button>
                  <span class="settings-value">{subtitleSettings.fontSize}px</span>
                  <button class="settings-btn" on:click|stopPropagation={() => updateSubtitleSettings('fontSize', Math.min(48, subtitleSettings.fontSize + 2))}>+</button>
                </div>
                <button class="settings-reset-btn" title="Reset" on:click|stopPropagation={() => resetSubtitleSetting('fontSize')}>
                  <i class="ri-refresh-line"></i>
                </button>
              </div>
            </div>
            
            <div class="settings-group">
              <label>Color</label>
              <div class="settings-controls-wrapper">
                <div class="settings-controls">
                  <input 
                    type="color" 
                    class="settings-color-input"
                    value={subtitleSettings.color} 
                    on:input={(e) => updateSubtitleSettings('color', e.target.value)}
                  />
                </div>
                <button class="settings-reset-btn" title="Reset" on:click|stopPropagation={() => resetSubtitleSetting('color')}>
                  <i class="ri-refresh-line"></i>
                </button>
              </div>
            </div>

            <div class="settings-group">
              <label>Shadow</label>
              <div class="settings-controls-wrapper">
                <div class="settings-controls">
                  <button 
                    class="settings-btn toggle-btn" 
                    class:active={subtitleSettings.textShadow}
                    on:click|stopPropagation={() => updateSubtitleSettings('textShadow', !subtitleSettings.textShadow)}
                  >
                    <i class={subtitleSettings.textShadow ? "ri-checkbox-circle-fill" : "ri-checkbox-blank-circle-line"}></i>
                  </button>
                  {#if subtitleSettings.textShadow}
                    <input 
                      type="color" 
                      class="settings-color-input"
                      value={subtitleSettings.textShadowColor} 
                      on:input={(e) => updateSubtitleSettings('textShadowColor', e.target.value)}
                    />
                  {/if}
                </div>
                <button class="settings-reset-btn" title="Reset" on:click|stopPropagation={() => {
                  resetSubtitleSetting('textShadow');
                  resetSubtitleSetting('textShadowColor');
                }}>
                  <i class="ri-refresh-line"></i>
                </button>
              </div>
            </div>
            
            <div class="settings-group">
              <label>Background</label>
              <div class="settings-controls-wrapper">
                <div class="settings-controls">
                  <button class="settings-btn" on:click|stopPropagation={() => updateSubtitleSettings('backgroundOpacity', Math.max(0, parseFloat((subtitleSettings.backgroundOpacity - 0.1).toFixed(1))))}>-</button>
                  <span class="settings-value">{Math.round(subtitleSettings.backgroundOpacity * 100)}%</span>
                  <button class="settings-btn" on:click|stopPropagation={() => updateSubtitleSettings('backgroundOpacity', Math.min(1, parseFloat((subtitleSettings.backgroundOpacity + 0.1).toFixed(1))))}>+</button>
                </div>
                <button class="settings-reset-btn" title="Reset" on:click|stopPropagation={() => resetSubtitleSetting('backgroundOpacity')}>
                  <i class="ri-refresh-line"></i>
                </button>
              </div>
            </div>

            <div class="settings-group">
              <label>Offset</label>
              <div class="settings-controls-wrapper">
                <div class="settings-controls">
                  <button class="settings-btn wide-btn" on:click|stopPropagation={() => updateSubtitleOffset(-0.1)}>
                    <i class="ri-subtract-line"></i> Audio First
                  </button>
                  <span class="settings-value" style="min-width: 50px;">
                    {Math.abs(subtitleOffset).toFixed(1)}s
                  </span>
                  <button class="settings-btn wide-btn" on:click|stopPropagation={() => updateSubtitleOffset(0.1)}>
                    Sub First <i class="ri-add-line"></i>
                  </button>
                </div>
                <button class="settings-reset-btn" title="Reset" on:click|stopPropagation={() => resetSubtitleOffset()}>
                  <i class="ri-refresh-line"></i>
                </button>
              </div>
            </div>

            <div class="settings-group">
              <label>Position</label>
              <div class="settings-controls-wrapper">
                <div class="settings-controls">
                  <button class="settings-btn" on:click|stopPropagation={() => updateSubtitleSettings('windowMargin', Math.max(0, subtitleSettings.windowMargin - 10))}>↓</button>
                  <span class="settings-value">{subtitleSettings.windowMargin}px</span>
                  <button class="settings-btn" on:click|stopPropagation={() => updateSubtitleSettings('windowMargin', Math.min(200, subtitleSettings.windowMargin + 10))}>↑</button>
                </div>
                <button class="settings-reset-btn" title="Reset" on:click|stopPropagation={() => resetSubtitleSetting('windowMargin')}>
                  <i class="ri-refresh-line"></i>
                </button>
              </div>
            </div>
          </div>
        {/if}
      </div>

      <button on:click={toggleFullscreen} class="fullscreen-btn control-btn">
        <i class={fullscreen ? "ri-fullscreen-exit-line" : "ri-fullscreen-line"}
        ></i>
      </button>
    </div>
  </div>

  <!-- Episodes Panel -->
  {#if showEpisodesPanel && metadata && metadata.seasons}
    <!-- svelte-ignore a11y-click-events-have-key-events -->
    <!-- svelte-ignore a11y-no-static-element-interactions -->
    <div class="episodes-island-backdrop" on:click={() => showEpisodesPanel = false}></div>
    <!-- svelte-ignore a11y-click-events-have-key-events -->
    <!-- svelte-ignore a11y-no-static-element-interactions -->
    <div class="episodes-island" on:click|stopPropagation>
      <div class="episodes-panel-header">
        <span class="episodes-panel-title">{metadata.name || metadata.title}</span>
        <button class="episodes-panel-close" on:click={() => showEpisodesPanel = false} title="Close">
          <i class="ri-close-line"></i>
        </button>
      </div>

      <!-- Season tabs -->
      {#if metadata.seasons.filter(s => s.season_number > 0).length > 1}
        <div class="episodes-season-tabs">
          {#each metadata.seasons.filter(s => s.season_number > 0) as season}
            <button
              class="episodes-season-tab"
              class:active={episodesPanelSeason === season.season_number}
              on:click={() => loadSeasonEpisodes(season.season_number)}
            >S{season.season_number}</button>
          {/each}
        </div>
      {/if}

      <!-- Episode list -->
      <div class="episodes-list-scroll">
        {#if loadingEpisodesPanel}
          <div class="episodes-loading">
            <div class="episodes-loading-spinner"></div>
          </div>
        {:else if episodesData[episodesPanelSeason]}
          {#each episodesData[episodesPanelSeason].episodes as ep}
            {@const epKey = `${mediaId}-${mediaType}-S${episodesPanelSeason}-E${ep.episode_number}`}
            {@const epProgress = $watchProgressStore[epKey]}
            {@const epPct = epProgress?.duration ? (epProgress.currentTimestamp / epProgress.duration) * 100 : 0}
            {@const isCurrent = episodesPanelSeason === seasonNum && ep.episode_number === episodeNum}
            <!-- svelte-ignore a11y-click-events-have-key-events -->
            <!-- svelte-ignore a11y-no-static-element-interactions -->
            <div
              class="episodes-panel-item"
              class:current={isCurrent}
              on:click={() => playEpisodeFromPanel(episodesPanelSeason, ep.episode_number)}
            >
              <div class="episodes-panel-still">
                {#if ep.still_path}
                  <img src={getImageUrl(ep.still_path, 'w300')} alt={ep.name} />
                {:else}
                  <div class="episodes-panel-still-placeholder"><i class="ri-film-line"></i></div>
                {/if}
                {#if isCurrent}
                  <div class="episodes-panel-now-playing"><i class="ri-play-fill"></i></div>
                {:else if epPct > 85}
                  <div class="episodes-panel-watched"><i class="ri-check-line"></i></div>
                {:else if epPct > 0}
                  <div class="episodes-panel-progress-bar"><div style="width:{epPct}%"></div></div>
                {/if}
              </div>
              <div class="episodes-panel-info">
                <span class="episodes-panel-num">E{ep.episode_number}</span>
                <span class="episodes-panel-name">{ep.name}</span>
              </div>
            </div>
          {/each}
        {/if}
      </div>
    </div>
  {/if}

  <!-- External Player Overlay -->
  {#if playingInExternal}    <div class="external-player-overlay">
      <div class="external-player-content">
        <i class="ri-external-link-line external-icon"></i>
        <h2>Playing in External Player</h2>
        <p>Video is being played in your external media player</p>
        <div class="external-actions">
          <button class="btn-standard" on:click={restoreInternalPlayer}>
            <i class="ri-play-circle-line"></i> Restore Integrated Player
          </button>
          <button class="btn-standard" on:click={goToNextEpisodeMenu} disabled={!seasonNum || !episodeNum}>
            <i class="ri-skip-forward-line"></i> Next Episode
          </button>
        </div>
      </div>
    </div>
  {/if}
</div>

<!-- styles moved to src/styles/main.css -->
