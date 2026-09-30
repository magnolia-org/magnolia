<script>
  import { onMount, onDestroy } from "svelte";
  import { fade, scale } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import {
    getMovieDetails,
    getTVDetails,
    getSeasonDetails,
    getImageUrl,
    getCorsImageUrl,
    getTVExternalIds,
    getMovieExternalIds,
  } from "./tmdb.js";
  import { myListStore } from "./stores/listStore.js";
  import { watchProgressStore } from "./stores/watchProgressStore.js";
import { invoke } from "@tauri-apps/api/core";
  import TorrentSelector from "./TorrentSelector.svelte";
  import FileSelector from "./FileSelector.svelte";
  import ErrorModal from "./ErrorModal.svelte";
  import TorrentManager from "./TorrentManager.svelte";
  import SubtitlePackManager from "./SubtitlePackManager.svelte";
  import Scroller from "./Scroller.svelte";
  import MediaCarousel from "./MediaCarousel.svelte";

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
  let detailsToken = 0;
  let recommendations = [];
  let keywords = [];
  let activeTab = "";
  let availableTabs = [];
  let viewMode = "list"; // 'list' or 'grid'
  let episodeSearchQuery = "";

  let showTorrentSelector = false;
  let isOperationCancelled = false;
  let isSelectingTorrent = false;
  let searchResults = [];
  let isSearching = false;
  let currentSearchQuery = "";
  let originalSearchQuery = "";
  let pendingPlayRequest = null;
  let currentImdbId = null;
  
  let showFileSelector = false;
  let availableFiles = [];
  let selectedTorrentForManual = null;
  let selectedTorrentName = "";
  let manualHandleId = null;
  let autoPlayedMedia = null;

  let showErrorModal = false;
  let errorMessage = "";
  let errorTitle = "";
  let errorDetails = "";
  let errorAction = null;

  let showTorrentManager = false;
  let showSubtitlePackManager = false;
  let showMoreMenu = false;
  let torrentManagerRefresh = 0;
  let torrentFileCache = {};
  let isPlayLoading = false;
  let detectedIsAnime = false;

  // fetch the video files of a magnet so the selection ui/auto-matcher can run.
  // the backend asks the debrid client when one is active (no local torrent,
  // handleId is null), otherwise librqbit resolves the metadata.
  async function fetchTorrentMetadata(magnetLink) {
    const result = await invoke("get_torrent_files", {
      magnetLink,
      season: pendingPlayRequest?.season ?? null,
      episode: pendingPlayRequest?.episode ?? null,
      mediaType: media.media_type ?? null,
    });
    return { handleId: result.handle_id, info: { files: result.files } };
  }

  $: {
    if (media && !media.media_type) {
      media.media_type = media.name && !media.title ? "tv" : "movie";
    }
  }

  $: isInMyList =
    media &&
    $myListStore.some(
      (item) => item.id === media.id && item.media_type === media.media_type,
    );

  // the backend checks the anime list and falls back to the animation genre
  async function checkIsAnime(forDetails) {
    try {
      const result = await invoke('check_is_anime', {
        tmdbId: forDetails.id,
        mediaType: media.media_type,
        genreIds: (forDetails.genres || []).map(genre => genre.id),
      });
      if (details === forDetails) detectedIsAnime = result;
    } catch (e) {
      detectedIsAnime = false;
    }
  }

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
    keywords = [];
    selectedTorrentName = ""; // Reset torrent name for new media
    isPlayLoading = false;
    detectedIsAnime = false;
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
      const firstSeason = details.seasons.find((s) => s.season_number > 0);
      if (firstSeason) {
        selectedSeason = firstSeason.season_number;
      }
      loadAllSeasons();
    } else {
      activeTab = "cast";
    }

    loadRecommendations();
    checkIsAnime(details);
  }

  $: if (selectedSeason && details) {
    loadSeasonDetails();
  }

  // trigger autoplay once per opened media, tracked by identity so the prop is never
  // mutated (mutating it reruns loadDetails and nulls details mid-play)
  $: if (details && media?.autoPlay && autoPlayedMedia !== media) {
    autoPlayedMedia = media;
    handleAutoPlay();
  }

  onMount(() => {
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
    const token = ++detailsToken;
    loading = true;
    details = null;
    credits = null;
    keywords = [];
    try {
      // credits, keywords and recommendations ride along on the details request
      const loaded = media.media_type === "movie"
        ? await getMovieDetails(media.id)
        : await getTVDetails(media.id);
      if (token !== detailsToken) return;

      credits = normalizeCredits(loaded);
      keywords = loaded?.keywords?.keywords || loaded?.keywords?.results || [];
      details = loaded;

      if (details && details.backdrop_path) {
        await extractColors(getCorsImageUrl(details.backdrop_path, "w300"));
      }
    } catch (err) {
      console.error("Error loading details:", err);
      if (token !== detailsToken) return;
      details = null;
    }
    loading = false;
  }

  // crew is shown by tier, then by how many episodes they worked on, each person once with all their jobs
  const CREW_TIERS = [
    ["Creator"],
    ["Director", "Screenplay", "Writer", "Teleplay", "Story", "Novel", "Characters"],
    ["Original Music Composer", "Music", "Director of Photography"],
    ["Producer", "Executive Producer", "Editor"],
  ];

  function jobRank(job) {
    const tier = CREW_TIERS.findIndex((jobs) => jobs.includes(job));
    return tier < 0 ? CREW_TIERS.length : tier;
  }

  function normalizeCredits(data) {
    const source = data?.aggregate_credits || data?.credits;
    if (!source) return null;

    const cast = (source.cast || []).slice(0, 30).map((person) => ({
      ...person,
      character: person.roles
        ? person.roles.map((role) => role.character).filter(Boolean).slice(0, 2).join(" / ")
        : person.character,
      episode_count: person.total_episode_count ?? person.episode_count,
    }));

    const crewById = new Map();
    for (const person of source.crew || []) {
      const jobs = person.jobs ? person.jobs.map((job) => job.job) : [person.job];
      const existing = crewById.get(person.id);
      if (existing) {
        existing.jobs = [...new Set([...existing.jobs, ...jobs])];
      } else {
        crewById.set(person.id, { ...person, jobs, episode_count: person.total_episode_count ?? person.episode_count });
      }
    }
    for (const creator of data?.created_by || []) {
      const existing = crewById.get(creator.id);
      if (existing) existing.jobs = ["Creator", ...existing.jobs.filter((job) => job !== "Creator")];
      else crewById.set(creator.id, { ...creator, jobs: ["Creator"] });
    }

    const crew = [...crewById.values()]
      .map((person) => ({ ...person, jobs: [...person.jobs].sort((a, b) => jobRank(a) - jobRank(b)) }))
      .sort((a, b) =>
        jobRank(a.jobs[0]) - jobRank(b.jobs[0]) ||
        (b.episode_count || 0) - (a.episode_count || 0) ||
        (b.popularity || 0) - (a.popularity || 0))
      .slice(0, 24);

    return { cast, crew };
  }

  function openPerson(person) {
    window.dispatchEvent(
      new CustomEvent("viewAll", {
        detail: { title: person.name, type: "all", category: "person", filterId: person.id },
      }),
    );
  }

  function openTagView(tag, tagType) {
    if (!tag?.id) return;

    window.dispatchEvent(
      new CustomEvent("viewAll", {
        detail: {
          title: tag.name,
          type: "all",
          category: tagType === "genre" ? "discover_by_genre" : "discover_by_keyword",
          filterId: tag.id,
        },
      }),
    );
    // The "viewAll" handler in App.svelte clears selectedMedia, so no
    // dispatch("close") needed — it would race with the new viewAllData state.
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

  // tmdb's recommendations are usually on point; similar titles only fill in when there are none
  function loadRecommendations() {
    const usable = (list) =>
      (list || []).filter((rec) => rec.poster_path && (rec.vote_count || 0) >= 10);
    const picks = usable(details?.recommendations?.results);
    recommendations = (picks.length > 0 ? picks : usable(details?.similar?.results))
      .slice(0, 20)
      .map((rec) => ({ ...rec, media_type: rec.media_type || media.media_type }));
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

        // Calculate darker color for titlebar to prevent it from being too vibrant/bright
        let tr = r, tg = g, tb = b;
        const brightness = (tr * 299 + tg * 587 + tb * 114) / 1000;
        const MAX_TITLEBAR_BRIGHTNESS = 40; // Keep it relatively dark

        if (brightness > MAX_TITLEBAR_BRIGHTNESS) {
            const factor = MAX_TITLEBAR_BRIGHTNESS / brightness;
            tr = Math.floor(tr * factor);
            tg = Math.floor(tg * factor);
            tb = Math.floor(tb * factor);
        }

        const hexColor = rgbToHex(tr, tg, tb);
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

  import { getRatingColor } from "./utils/colorUtils.js";
  import { formatTime } from "./utils/timeUtils.js";
  import { scrollHoverGuard } from "./utils/scrollHoverGuard.js";

  // action is an optional { label, icon, run } offered next to dismiss
  function showError(message, { title = "Something went wrong", error = null, action = null } = {}) {
    errorMessage = message;
    errorTitle = title;
    errorDetails = formatErrorDetails(error);
    errorAction = action;
    showErrorModal = true;
  }

  function formatErrorDetails(error) {
    if (!error) return "";
    if (typeof error === "string") return error;
    if (error instanceof Error) return error.stack || error.message;
    try {
      return JSON.stringify(error, null, 2);
    } catch {
      return String(error);
    }
  }

  function seasonList() {
    return (details?.seasons || []).map(s => ({
      season_number: s.season_number,
      episode_count: s.episode_count ?? 0,
    }));
  }

  // where to continue from, decided by the backend: a movie's saved position, or
  // the current episode (or the next one once it's watched). `progress` overrides
  // the saved position, e.g. "next episode" handed over from the player.
  async function getResumeInfo(progress = null) {
    if (!details) return null;
    const target = await invoke("get_resume_target", {
      mediaId: details.id,
      mediaType: media.media_type,
      seasons: seasonList(),
      progress,
    });
    if (!target) return null;
    if (target.season == null) {
      return { ...target, label: `Resume from ${formatTime(target.timestamp)}` };
    }
    const label = `S${target.season}E${target.episode}`;
    const hasTimestamp = target.timestamp > 60;
    return { ...target, label: `${label}${hasTimestamp ? ` • ${formatTime(target.timestamp)}` : ''}` };
  }

  let resumeInfo = null;
  let resumeRequest = 0;
  $: if (details) refreshResumeInfo($watchProgressStore[`${details.id}-${media.media_type}`]);
  $: if (!details) resumeInfo = null;

  async function refreshResumeInfo(_progress) {
    const request = ++resumeRequest;
    try {
      const info = await getResumeInfo();
      if (request === resumeRequest) resumeInfo = info;
    } catch (err) {
      console.error("failed to load resume info:", err);
    }
  }

  $: movieWatched = media?.media_type === 'movie' && !!$watchProgressStore[`${media.id}-movie`]?.watched;

  // True when the last episode of the last season has been watched >85%
  $: seriesWatched = (() => {
    if (!details?.seasons || media?.media_type !== 'tv') return false;
    const seasons = details.seasons.filter(s => s.season_number > 0);
    if (!seasons.length) return false;
    const lastSeason = seasons[seasons.length - 1];
    const lastSeasonData = allSeasonsData[lastSeason.season_number];
    if (!lastSeasonData?.episodes?.length) return false;
    const lastEp = lastSeasonData.episodes[lastSeasonData.episodes.length - 1];
    const key = `${details.id}-${media.media_type}-S${lastSeason.season_number}-E${lastEp.episode_number}`;
    return !!$watchProgressStore[key]?.watched;
  })();

  function isSeasonWatched(seasonNum) {
    const seasonData = allSeasonsData[seasonNum];
    if (!seasonData?.episodes?.length) return false;
    const lastEp = seasonData.episodes[seasonData.episodes.length - 1];
    const key = `${details.id}-${media.media_type}-S${seasonNum}-E${lastEp.episode_number}`;
    return !!$watchProgressStore[key]?.watched;
  }

  function toggleSeason(seasonNumber) {
    if (selectedSeason === seasonNumber) {
      selectedSeason = null;
    } else {
      selectedSeason = seasonNumber;
    }
  }

  async function handleAutoPlay() {
    console.log('Auto-play triggered');

    // use resumeProgress if passed from quick play, otherwise the saved progress.
    // skips past finished episodes, and is null once the whole series is done
    const next = await getResumeInfo(media.resumeProgress || null);

    if (media.media_type === 'movie') {
      // Movie: play from beginning (timestamp resume handled by VideoPlayer)
      await handlePlay(0, 0);
    } else if (next?.season && next?.episode) {
      console.log(`Resuming from S${next.season}E${next.episode} at ${next.timestamp}s`);
      await handlePlay(next.season, next.episode);
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
    isPlayLoading = true;
    isOperationCancelled = false;
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
    isPlayLoading = false;
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

    const isMovie = media.media_type === "movie" || !!details.title;
    currentSearchQuery = details.title || details.name;
    originalSearchQuery = currentSearchQuery;

    // fetch the imdb id: tracker extensions (e.g. eztv) may use it for lookup
    const imdbId = await fetchImdbId(isMovie);

    // the backend picks the trackers (saved preference, or anime/general in
    // auto mode), merges their results and ranks them for this episode
    try {
      const response = await runSearch({ seasonNum, episodeNum, imdbId, query: null, trackers: null });
      searchResults = response.results;
      originalSearchQuery = currentSearchQuery = response.query;

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

  function releaseYear() {
    const year = parseInt((details.release_date || details.first_air_date || "").split("-")[0]);
    return Number.isFinite(year) ? year : null;
  }

  async function fetchImdbId(isMovie) {
    try {
      const externalIds = isMovie
        ? await getMovieExternalIds(details.id)
        : await getTVExternalIds(details.id);
      if (externalIds?.imdb_id) {
        currentImdbId = externalIds.imdb_id;
        console.log("Got IMDB ID:", currentImdbId);
        return currentImdbId;
      }
    } catch (err) {
      console.warn("Failed to get IMDB ID:", err);
    }
    return null;
  }

  // trackers: null uses the saved preference, [] means auto
  async function runSearch({ seasonNum, episodeNum, imdbId, query, trackers }) {
    return await invoke("search_torrents", {
      request: {
        tmdbId: details.id,
        mediaType: media.media_type,
        title: details.title || details.name,
        query,
        genreIds: (details.genres || []).map(genre => genre.id),
        releaseYear: releaseYear(),
        season: seasonNum,
        episode: episodeNum,
        imdbId,
        trackers,
      },
    });
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

  async function openTmdbPage() {
    if (!media || !media.id) return;
    
    const mediaType = media.media_type === 'movie' ? 'movie' : 'tv';
    const tmdbUrl = `https://www.themoviedb.org/${mediaType}/${media.id}`;
    
    try {
      await invoke('open_external_url', { url: tmdbUrl });
    } catch (error) {
      console.error('Failed to open TMDB page:', error);
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
  
  async function handleTorrentManagerManualAssignment(event) {
    const { torrents, showId } = event.detail;
    
    if (!torrents || torrents.length === 0) {
      showError("There are no saved torrents for this title yet. Play an episode first, then assign files from its torrent.", { title: "Nothing to assign" });
      return;
    }
    
    // Close torrent manager and show loading
    showTorrentManager = false;
    showFileSelector = true;
    availableFiles = [];
    
    // Load files from first torrent initially
    const firstTorrent = torrents[0];
    try {
      // Check cache first
      let handleId;
      let files;
      
      if (torrentFileCache[firstTorrent.magnetLink]) {
        console.log('[cache] using cached file list for torrent');
        files = torrentFileCache[firstTorrent.magnetLink].files;
        handleId = torrentFileCache[firstTorrent.magnetLink].handleId;
      } else {
        console.log('[cache] fetching new file list for torrent');
        const meta = await fetchTorrentMetadata(firstTorrent.magnetLink);
        handleId = meta.handleId;
        files = meta.info.files;

        // Cache the results
        torrentFileCache[firstTorrent.magnetLink] = { files, handleId };
      }
      
      availableFiles = files;
      
      if (availableFiles.length === 0) {
        showError("None of the saved torrents contain video files Magnolia can play.", { title: "No video files" });
        return;
      }
      
      // Get current assignments
      const allSelections = await invoke("get_all_torrent_selections", {
        showId: details.id,
      });
      
      // Build current assignments map
      const currentAssignmentsMap = {};
      if (allSelections && allSelections.seasons) {
        for (const [seasonNum, seasonData] of Object.entries(allSelections.seasons)) {
          for (const [episodeNum, episodeData] of Object.entries(seasonData.episodes)) {
            if (episodeData.magnet_link === firstTorrent.magnetLink) {
              const key = `${seasonNum}-${episodeNum}`;
              currentAssignmentsMap[key] = {
                season: parseInt(seasonNum),
                episode: parseInt(episodeNum),
                fileIndex: episodeData.file_index
              };
            }
          }
        }
      }
      
      // Store all torrents for switching
      selectedTorrentForManual = { 
        ...firstTorrent,
        allTorrents: torrents,
        currentTorrentIndex: 0,
        currentAssignments: currentAssignmentsMap
      };
      manualHandleId = handleId;
      
    } catch (err) {
      console.error("error loading torrent for manual assignment:", err);
      showError("Magnolia couldn't read the files in this torrent. It may not have any seeders right now.", { title: "Couldn't load torrent", error: err });
    }
  }

  async function onTorrentSelect(event) {
    const torrent = event.detail;
    console.log("Selected torrent:", torrent);
    
    // Store the selected torrent name for display
    selectedTorrentName = torrent.title || "";

    if (!pendingPlayRequest) return;

    try {
      // 1. get the torrent's video files (from the debrid client when active,
      //    otherwise librqbit metadata). debrid mode has no local handle.
      const { handleId, info } = await fetchTorrentMetadata(torrent.magnet_link);

      console.log("Torrent info:", info);

      // 2. the backend finds the requested episode, remembers it and, for season
      //    packs, every other numbered episode too
      const matchedFile = await invoke("auto_assign_torrent_files", {
        showId: details.id,
        mediaType: media.media_type,
        season: pendingPlayRequest.season,
        episode: pendingPlayRequest.episode,
        magnetLink: torrent.magnet_link,
        files: info.files,
      });

      if (matchedFile) {
        console.log("Found matching file:", matchedFile.name);
        torrentManagerRefresh++;
        startStream(torrent.magnet_link, matchedFile.index, handleId);
        showTorrentSelector = false;
      } else {
        // show manual file selection when nothing matched
        showManualFileSelector(torrent, info, handleId);
      }
    } catch (err) {
      // Don't show error if user cancelled the operation
      if (isOperationCancelled) {
        console.log("torrent selection cancelled by user");
        isOperationCancelled = false;
        return;
      }
      console.error("Error processing selection:", err);
      showError("The torrent's file list never arrived. It may have too few seeders, so try again or pick a different source.", { title: "Couldn't load torrent", error: err });
    }
  }

  async function startStream(magnetLink, fileIndex, existingHandleId = null) {
    try {
      // the backend adds the torrent for the built-in client (a debrid client
      // streams remotely, so handleId stays null) and picks the start position.
      // if the torrent can't be added it forgets the saved selection, so the
      // next play asks for a new source instead of getting stuck.
      const isMovie = media.media_type === "movie" || !!details.title;
      const plan = await invoke("prepare_playback", {
        request: {
          mediaId: details.id,
          mediaType: media.media_type,
          season: isMovie ? null : pendingPlayRequest.season,
          episode: isMovie ? null : pendingPlayRequest.episode,
          magnetLink,
          fileIndex,
          handleId: existingHandleId,
        },
      });

      // Don't start stream here. Let VideoPlayer handle it so it can show loading screen.
      console.log("Opening player for handle:", plan.handle_id, "file:", fileIndex);

      // Build title - hide SXXEXX for movies
      const playerTitle = isMovie
        ? (details.title || details.name)
        : `${details.title || details.name} - S${pendingPlayRequest.season}E${pendingPlayRequest.episode}`;

      // Dispatch event to open video player
      isPlayLoading = false;
      window.dispatchEvent(
        new CustomEvent("openVideoPlayer", {
          detail: {
            src: null, // VideoPlayer will fetch this
            title: playerTitle,
            metadata: details, // Pass full details for watch history
            handleId: plan.handle_id,
            fileIndex: plan.file_index,
            magnetLink: plan.magnet_link,
            initialTimestamp: plan.initial_timestamp,
            mediaId: details.id,
            mediaType: media.media_type,
            seasonNum: plan.season,
            episodeNum: plan.episode,
          },
        }),
      );
      return true;
    } catch (err) {
      console.error("Error preparing stream:", err);
      torrentManagerRefresh++;
      isPlayLoading = false;
      const failedRequest = pendingPlayRequest;
      showError("The saved torrent for this title couldn't be started. It may have been removed or have no seeders left.", {
        title: "Couldn't start playback",
        error: err,
        action: failedRequest && {
          label: "Choose another source",
          icon: "ri-search-line",
          run: () => handlePlay(failedRequest.season, failedRequest.episode, true),
        },
      });
      return false;
    }
  }

  function closeTorrentSelector() {
    isOperationCancelled = true;
    isPlayLoading = false;
    isSelectingTorrent = false;
    showTorrentSelector = false;
    pendingPlayRequest = null;
  }

  const handleResearch = async (event) => {
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
    
    console.log("New trackers:", trackers);
    console.log("Custom query:", customQuery);
    console.log("Use IMDB:", useImdb);
    
    // Re-run the search with new tracker preference
    isSearching = true;
    searchResults = [];

    const isMovieCheck = media.media_type === "movie" || !!details.title;
    const { season: seasonNum, episode: episodeNum } = pendingPlayRequest;

    // If useImdb is true, re-fetch IMDB ID for EZTV
    let imdbIdToUse = currentImdbId;
    if (useImdb && !imdbIdToUse) {
      imdbIdToUse = await fetchImdbId(isMovieCheck);
    }

    try {
      const response = await runSearch({
        seasonNum,
        episodeNum,
        imdbId: imdbIdToUse,
        query: customQuery || null,
        trackers: trackers || [],
      });
      searchResults = response.results;
      currentSearchQuery = response.query;

      console.log(`Found ${searchResults.length} results`);
    } catch (err) {
      console.error("Error during research:", err);
      searchResults = [];
    } finally {
      isSearching = false;
    }
  };

  function showManualFileSelector(torrent, info, handleId) {
    // the backend only lists video files
    availableFiles = info.files;

    if (availableFiles.length === 0) {
      showError("This torrent doesn't contain any video files Magnolia can play. Pick a different source.", { title: "No video files" });
      return;
    }
    
    selectedTorrentForManual = {
      ...torrent,
      allTorrents: [{ magnetLink: torrent.magnet_link, fileName: torrent.title }],
      currentTorrentIndex: 0
    };
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

    const magnetLink = selectedTorrentForManual.magnet_link || selectedTorrentForManual.magnetLink;

    try {
      for (const { file, season, episode } of assignments) {
        console.log(`saving S${season}E${episode} for file: ${file.name}`);
        await invoke("save_torrent_selection", {
          showId: details.id,
          season: season,
          episode: episode,
          magnetLink: magnetLink,
          fileIndex: file.index,
        });
      }

      torrentManagerRefresh++;

      let playAssignment = assignments.find(
        a => a.season === pendingPlayRequest?.season && a.episode === pendingPlayRequest?.episode
      );
      
      if (!playAssignment) {
        playAssignment = assignments[0];
        if (pendingPlayRequest) {
          pendingPlayRequest = { season: playAssignment.season, episode: playAssignment.episode };
        }
      }

      if (pendingPlayRequest) {
        startStream(magnetLink, playAssignment.file.index, manualHandleId);
      }
      closeFileSelector();
    } catch (err) {
      console.error("error saving file selections:", err);
      showError("Your file assignments couldn't be saved.", { title: "Couldn't save", error: err });
    }
  }

  function closeFileSelector() {
    showFileSelector = false;
    availableFiles = [];
    selectedTorrentForManual = null;
    manualHandleId = null;
  }
  
  async function handleFileSelectorSwitchTorrent(event) {
    const { index } = event.detail;
    
    if (!selectedTorrentForManual?.allTorrents || !selectedTorrentForManual.allTorrents[index]) {
      return;
    }
    
    const newTorrent = selectedTorrentForManual.allTorrents[index];
    
    // Set loading state
    selectedTorrentForManual = {
      ...selectedTorrentForManual,
      isSwitchingTorrent: true
    };
    
    try {
      // Check cache first
      let handleId;
      let files;
      
      if (torrentFileCache[newTorrent.magnetLink]) {
        console.log('[cache] using cached file list for torrent');
        files = torrentFileCache[newTorrent.magnetLink].files;
        handleId = torrentFileCache[newTorrent.magnetLink].handleId;
      } else {
        console.log('[cache] fetching new file list for torrent');
        const meta = await fetchTorrentMetadata(newTorrent.magnetLink);
        handleId = meta.handleId;
        files = meta.info.files;

        // Cache the results
        torrentFileCache[newTorrent.magnetLink] = { files, handleId };
      }
      
      availableFiles = files;
      
      // Get current assignments for this torrent
      const allSelections = await invoke("get_all_torrent_selections", {
        showId: details.id,
      });
      
      // Build current assignments map for this torrent
      const currentAssignmentsMap = {};
      if (allSelections && allSelections.seasons) {
        for (const [seasonNum, seasonData] of Object.entries(allSelections.seasons)) {
          for (const [episodeNum, episodeData] of Object.entries(seasonData.episodes)) {
            if (episodeData.magnet_link === newTorrent.magnetLink) {
              const key = `${seasonNum}-${episodeNum}`;
              currentAssignmentsMap[key] = {
                season: parseInt(seasonNum),
                episode: parseInt(episodeNum),
                fileIndex: episodeData.file_index
              };
            }
          }
        }
      }
      
      selectedTorrentForManual = {
        ...newTorrent,
        allTorrents: selectedTorrentForManual.allTorrents,
        currentTorrentIndex: index,
        currentAssignments: currentAssignmentsMap,
        isSwitchingTorrent: false
      };
      manualHandleId = handleId;
      
    } catch (err) {
      console.error("error switching torrent:", err);
      showError("Magnolia couldn't read the files in the selected torrent. It may not have any seeders right now.", { title: "Couldn't load torrent", error: err });
      selectedTorrentForManual = {
        ...selectedTorrentForManual,
        isSwitchingTorrent: false
      };
    }
  }
</script>

<!-- svelte-ignore a11y-click-events-have-key-events -->
<!-- svelte-ignore a11y-no-static-element-interactions -->
<div class="detail-overlay" style="animation: fadeIn 0.3s ease;">
  <div
    class="detail-container"
    use:scrollHoverGuard
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
      <div class="detail-skeleton">
        <div class="skeleton-backdrop"></div>
        <div class="detail-content">
          <div class="detail-header">
            <div class="skeleton-poster skeleton-pulse"></div>
            <div class="skeleton-info-wrapper">
              <div class="skeleton-title skeleton-pulse"></div>
              <div class="skeleton-tagline skeleton-pulse"></div>
              <div class="skeleton-meta-row">
                <div class="skeleton-meta-chip skeleton-pulse"></div>
                <div class="skeleton-meta-chip skeleton-pulse"></div>
                <div class="skeleton-meta-chip skeleton-pulse"></div>
                <div class="skeleton-meta-chip skeleton-pulse"></div>
              </div>
              <div class="skeleton-genres-row">
                <div class="skeleton-genre skeleton-pulse"></div>
                <div class="skeleton-genre skeleton-pulse"></div>
                <div class="skeleton-genre skeleton-pulse"></div>
              </div>
              <div class="skeleton-overview">
                <div class="skeleton-line skeleton-pulse" style="width: 100%"></div>
                <div class="skeleton-line skeleton-pulse" style="width: 92%"></div>
                <div class="skeleton-line skeleton-pulse" style="width: 96%"></div>
                <div class="skeleton-line skeleton-pulse" style="width: 85%"></div>
                <div class="skeleton-line skeleton-pulse" style="width: 78%"></div>
                <div class="skeleton-line skeleton-pulse" style="width: 60%"></div>
              </div>
              <div class="skeleton-actions-row">
                <div class="skeleton-play-btn skeleton-pulse"></div>
                <div class="skeleton-btn skeleton-pulse"></div>
                <div class="skeleton-btn-icon skeleton-pulse"></div>
              </div>
            </div>
          </div>
        </div>
      </div>
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
                <div class="rating-box" style="background: {getRatingColor(details.vote_average)}">
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
                    <button
                      class="genre-tag genre-tag-large genre-tag-button"
                      type="button"
                      on:click={() => openTagView(genre, "genre")}
                    >
                      {genre.name}
                    </button>
                  {/each}
                </div>
              {/if}

              <p class="detail-overview detail-overview-large">
                {details.overview}
              </p>

              <div class="detail-actions">
                <button
                  class="btn-standard primary btn-large play-btn-with-resume"
                  disabled={isPlayLoading}
                  on:click={() => {
                    if (details.seasons && details.seasons.length > 0) {
                      if (seriesWatched) {
                        handlePlay(1, 1);
                      } else if (resumeInfo && resumeInfo.season && resumeInfo.episode) {
                        handlePlay(resumeInfo.season, resumeInfo.episode);
                      } else {
                        handlePlay(1, 1);
                      }
                    } else {
                      handlePlay(0, 0);
                    }
                  }}
                >
                  {#if isPlayLoading}
                    <i class="ri-loader-4-line spin"></i>
                  {:else if seriesWatched || movieWatched}
                    <i class="ri-repeat-line"></i>
                  {:else}
                    <i class="ri-play-fill"></i>
                  {/if}
                  <div class="play-btn-content">
                    <span class="play-btn-main">{seriesWatched || movieWatched ? 'Rewatch' : (resumeInfo ? 'Resume' : 'Play')}</span>
                    {#if seriesWatched && !isPlayLoading}
                      <span class="play-btn-resume-info">From S1E1</span>
                    {:else if movieWatched && !isPlayLoading}
                      <span class="play-btn-resume-info">From the start</span>
                    {:else if resumeInfo && !isPlayLoading}
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
                      {#if details?.seasons && details.seasons.length > 0}
                        <button class="menu-item" on:click={() => { showSubtitlePackManager = true; showMoreMenu = false; }}>
                          <i class="ri-closed-captioning-line"></i>
                          <span>Subtitle Packs</span>
                        </button>
                      {/if}
                      <button class="menu-item" on:click={() => { openTmdbPage(); showMoreMenu = false; }}>
                        <i class="ri-external-link-line"></i>
                        <span>View on TMDB</span>
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
                        <span class="season-name">Season {season.season_number}</span>
                        {#if isSeasonWatched(season.season_number)}
                          <span class="season-watched-badge">
                            <i class="ri-checkbox-circle-fill"></i>
                            Watched
                          </span>
                        {:else}
                          <span class="episode-count">{season.episode_count} Episodes</span>
                        {/if}
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
                              {@const isWatched = !!episodeProgress?.watched}
                              
                              <button
                                type="button"
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
                                      loading="lazy"
                                      decoding="async"
                                    />
                                  {:else}
                                    <div class="episode-placeholder">
                                      <i class="ri-film-line"></i>
                                    </div>
                                  {/if}
                                  
                                  {#if isWatched}
                                    <div class="watched-overlay">
                                      <i class="ri-checkbox-circle-fill"></i>
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
                                    <span class="episode-name" class:ep-watched-name={isWatched}>{episode.name}</span>
                                  </div>
                                  <div class="episode-meta">
                                    {#if episode.vote_average}
                                      <span class="episode-rating" style="background: {getRatingColor(episode.vote_average)}">
                                        {episode.vote_average.toFixed(1)}
                                      </span>
                                    {/if}
                                    <span class="episode-date">{formatDate(episode.air_date)}</span>
                                    {#if episode.runtime}
                                      <span>{episode.runtime}m</span>
                                    {/if}
                                  </div>
                                  <p class="episode-overview" class:ep-watched-overview={isWatched}>{episode.overview}</p>
                                </div>
                              </button>
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
                            <button
                              type="button"
                              class="heatmap-cell loaded"
                              style="background: {episode.vote_average ? getRatingColor(episode.vote_average) : 'rgba(255,255,255,0.12)'}"
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
                            </button>
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
              {#if credits.cast.length > 0}
                <div class="cast-section">
                  <h3 class="section-subtitle">Cast</h3>
                  <Scroller gap="var(--spacing-lg)">
                    {#each credits.cast as person (person.id)}
                      <button type="button" class="cast-card" on:click={() => openPerson(person)} title="See titles with {person.name}">
                        {#if person.profile_path}
                          <img
                            src={getImageUrl(person.profile_path, "w185")}
                            alt={person.name}
                            loading="lazy"
                            decoding="async"
                          />
                        {:else}
                          <div class="cast-placeholder">
                            <i class="ri-user-line"></i>
                          </div>
                        {/if}
                        <div class="cast-info">
                          <h4>{person.name}</h4>
                          {#if person.character}
                            <p>{person.character}</p>
                          {/if}
                          {#if person.episode_count}
                            <span class="episode-count">{person.episode_count} {person.episode_count === 1 ? "episode" : "episodes"}</span>
                          {/if}
                        </div>
                      </button>
                    {/each}
                  </Scroller>
                </div>
              {/if}

              {#if credits.crew.length > 0}
                <div class="crew-section">
                  <h3 class="section-subtitle">Crew</h3>
                  <Scroller gap="var(--spacing-lg)">
                    {#each credits.crew as person (person.id)}
                      <button type="button" class="cast-card crew-card" on:click={() => openPerson(person)} title="See titles with {person.name}">
                        {#if person.profile_path}
                          <img
                            src={getImageUrl(person.profile_path, "w185")}
                            alt={person.name}
                            loading="lazy"
                            decoding="async"
                          />
                        {:else}
                          <div class="cast-placeholder">
                            <i class="ri-user-line"></i>
                          </div>
                        {/if}
                        <div class="cast-info">
                          <h4>{person.name}</h4>
                          <p class="crew-job">{person.jobs.slice(0, 2).join(", ")}</p>
                          {#if person.episode_count}
                            <span class="episode-count">{person.episode_count} {person.episode_count === 1 ? "episode" : "episodes"}</span>
                          {/if}
                        </div>
                      </button>
                    {/each}
                  </Scroller>
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
              {#if details.genres && details.genres.length > 0}
                <div class="detail-item full-width">
                  <span class="label">Genres</span>
                  <div class="value detail-tags">
                    {#each details.genres as genre}
                      <button
                        class="detail-tag-btn"
                        type="button"
                        on:click={() => openTagView(genre, "genre")}
                      >
                        {genre.name}
                      </button>
                    {/each}
                  </div>
                </div>
              {/if}
              {#if keywords && keywords.length > 0}
                <div class="detail-item full-width">
                  <span class="label">Keywords</span>
                  <div class="value detail-tags">
                    {#each keywords as keyword}
                      <button
                        class="detail-tag-btn"
                        type="button"
                        on:click={() => openTagView(keyword, "keyword")}
                      >
                        {keyword.name}
                      </button>
                    {/each}
                  </div>
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
            {#key media.id}
              <MediaCarousel
                title="More Like This"
                customItems={recommendations}
                accentColor={prominentColor}
                fallbackMediaType={media.media_type}
                hideViewAll={true}
                watchProgress={$watchProgressStore}
              />
            {/key}
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
  <div class="episode-modal-overlay" on:click={() => (selectedEpisode = null)} transition:fade={{ duration: 400, easing: cubicOut }}>
    <!-- svelte-ignore a11y-click-events-have-key-events -->
    <!-- svelte-ignore a11y-no-static-element-interactions -->
    <div class="episode-modal" on:click={(e) => e.stopPropagation()} in:scale|global={{ start: 1.08, opacity: 0, duration: 400, easing: cubicOut }} out:scale|global={{ start: 1.08, opacity: 1, duration: 400, easing: cubicOut }}>
      <div class="episode-modal-header">
        <h3>
          Episode {selectedEpisode.episode_number}: {selectedEpisode.name}
        </h3>
        <div class="episode-modal-actions">
          <button
            class="btn-standard"
            disabled={isPlayLoading}
            on:click={() =>
              handlePlay(selectedSeason, selectedEpisode.episode_number)}
          >
            {#if isPlayLoading}
              <i class="ri-loader-4-line spin"></i>
            {:else}
              <i class="ri-play-fill"></i>
            {/if}
            Play
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
                  class="rating-badge"
                  style="background: {getRatingColor(selectedEpisode.vote_average)}"
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
    details={errorDetails}
    actionLabel={errorAction?.label || ""}
    actionIcon={errorAction?.icon}
    on:action={() => errorAction?.run()}
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
    isAnime={detectedIsAnime}
    hasImdbId={!!currentImdbId}
    isMovie={media.media_type === 'movie'}
currentSeason={pendingPlayRequest?.season}
    currentEpisode={pendingPlayRequest?.episode}
    bind:isSelectingTorrent
    on:select={onTorrentSelect}
    on:close={closeTorrentSelector}
    on:cancelSelection={closeTorrentSelector}
    on:research={handleResearch}
  />
{/if}

{#if showFileSelector}
  <FileSelector
    files={availableFiles}
    showName={details.title || details.name}
    seasons={details.seasons || []}
    isMovie={media.media_type === 'movie'}
    availableTorrents={selectedTorrentForManual?.allTorrents || []}
    currentTorrentIndex={selectedTorrentForManual?.currentTorrentIndex || 0}
    currentAssignments={selectedTorrentForManual?.currentAssignments || {}}
    isLoading={availableFiles.length === 0 && showFileSelector}
    isSwitchingTorrent={selectedTorrentForManual?.isSwitchingTorrent || false}
    on:confirm={handleFileSelectorConfirm}
    on:close={closeFileSelector}
    on:switchTorrent={handleFileSelectorSwitchTorrent}
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
    on:openManualAssignment={handleTorrentManagerManualAssignment}
  />
{/if}

{#if showSubtitlePackManager}
  <SubtitlePackManager
    {media}
    {details}
    {allSeasonsData}
    on:close={() => (showSubtitlePackManager = false)}
  />
{/if}
