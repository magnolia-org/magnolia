<script>
  import { onMount } from "svelte";
  import TitleBar from "./lib/TitleBar.svelte";
  import MediaCarousel from "./lib/MediaCarousel.svelte";
  import MediaDetail from "./lib/MediaDetail.svelte";
  import ViewAll from "./lib/ViewAll.svelte";
  import RecommendationsCarousel from "./lib/RecommendationsCarousel.svelte";
  import TorrentDebug from "./lib/TorrentDebug.svelte";
  import VideoPlayer from "./lib/VideoPlayer.svelte";
  import Onboarding from "./lib/Onboarding.svelte";
  import CacheManager from "./lib/CacheManager.svelte";
  import AboutModal from "./lib/AboutModal.svelte";
  import ExtensionManager from "./lib/ExtensionManager.svelte";
  import Updater from "./lib/Updater.svelte";
  import WindowResizeHandles from "./lib/WindowResizeHandles.svelte";
  import ConfirmModal from "./lib/ConfirmModal.svelte";
  import { myListStore } from "./lib/stores/listStore.js";
  import { watchHistoryStore } from "./lib/stores/watchHistoryStore.js";
  import { watchProgressStore } from "./lib/stores/watchProgressStore.js";
  import { modalStore, closeModal } from "./lib/stores/modalStore.js";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { invoke } from "@tauri-apps/api/core";
  import { setupLogging } from "./lib/consoleLogger.js";
  import { isLinux } from "./lib/utils/platform.js";
  import { scrollHoverGuard } from "./lib/utils/scrollHoverGuard.js";
  import { HOME_ROWS } from "./lib/browseLists.js";
  
  // Initialize console logging to disk
  setupLogging();

  let searchActive = false;
  let settingsActive = false;
  let selectedMedia = null;
  let viewAllData = null;
  let titleBarAccentColor = null;
  let mediaHistory = [];
  let historyIndex = -1;
  let showTorrentDebug = false;
  let savedScrollPosition = 0;
  let hideRecommendations = false;
  let confirmClearHistory = false;

  // Video Player State
  let showVideoPlayer = false;
  let videoPlayerProps = null;
  let videoControlsVisible = true;
  let pipMode = false;
  let onboardingVisible = false;

  $: myList = $myListStore;
  $: watchHistory = $watchHistoryStore;
  $: watchProgress = $watchProgressStore;
  $: activeModal = $modalStore.activeModal;

  $: document.body.classList.toggle('video-active', showVideoPlayer);

  onMount(async () => {
    try {
      const settings = await invoke('get_settings');
      hideRecommendations = settings.hide_recommendations;
    } catch (e) {
      console.error('Failed to load settings', e);
    }

    // Listen for settings changes
    window.addEventListener('settingsChanged', async (e) => {
      if (e.detail && e.detail.hide_recommendations !== undefined) {
        hideRecommendations = e.detail.hide_recommendations;
      }
    });

    window.addEventListener("openMediaDetail", (e) => {
      openMedia(e.detail);
    });

    window.addEventListener("updateTitleBarColor", (e) => {
      titleBarAccentColor = e.detail.color;
    });

    window.addEventListener("viewAll", (e) => {
      if (!viewAllData) {
        const scrollContainer = document.getElementById('main-content');
        if (scrollContainer) {
          savedScrollPosition = scrollContainer.scrollTop;
        }
      }
      // Clear media history so pressing Back from the genre/keyword page
      // always returns to home, not a previous detail view.
      selectedMedia = null;
      mediaHistory = [];
      historyIndex = -1;
      titleBarAccentColor = null;
      viewAllData = e.detail;
    });

    window.addEventListener("openVideoPlayer", async (e) => {
      console.log("[torrent] opening video player with handleId:", e.detail.handleId, "magnet:", e.detail.magnetLink?.substring(0, 50));
      
      // Wipe all torrent files before starting new stream
      try {
        console.log("[torrent] wiping all torrent files before starting stream");
        await invoke("wipe_all_torrent_files");
        console.log("[torrent] all torrent files wiped successfully");
      } catch (error) {
        console.error("[torrent] failed to wipe files:", error);
      }
      
      // If video player is already open, close it first to force remount
      if (showVideoPlayer) {
        showVideoPlayer = false;
        // Wait for next tick to ensure component is unmounted
        setTimeout(() => {
          videoPlayerProps = e.detail;
          showVideoPlayer = true;
          videoControlsVisible = true;
        }, 50);
      } else {
        videoPlayerProps = e.detail;
        showVideoPlayer = true;
        videoControlsVisible = true;
      }
    });

    window.addEventListener("videoControlsVisibility", (e) => {
      videoControlsVisible = e.detail.visible;
    });

    // the mini player owns the whole window, so the titlebar steps aside
    window.addEventListener("videoPipMode", (e) => {
      pipMode = e.detail.active;
    });

    window.addEventListener("mouseup", (e) => {
      if (e.button === 3) {
        // Back button
        e.preventDefault();
        navigateBack();
      } else if (e.button === 4) {
        // Forward button
        e.preventDefault();
        navigateForward();
      }
    });

    const handleKeyDown = (e) => {
      if (e.target.tagName === "INPUT" || e.target.tagName === "TEXTAREA")
        return;

      switch (e.key) {
        case "Escape":
          if (selectedMedia) {
            e.preventDefault();
            navigateBack();
          } else if (viewAllData) {
            e.preventDefault();
            navigateBack();
          } else if (showTorrentDebug) {
            e.preventDefault();
            showTorrentDebug = false;
          } else if (showVideoPlayer) {
            // VideoPlayer handles its own escape usually, but we can force close if needed
            // For now let's rely on the component's close event
          }
          break;
        case "ArrowLeft":
          if (selectedMedia && (e.altKey || e.metaKey)) {
            e.preventDefault();
            navigateBack();
          }
          break;
        case "ArrowRight":
          if (selectedMedia && (e.altKey || e.metaKey)) {
            e.preventDefault();
            navigateForward();
          }
          break;
        case "Home":
          if (!selectedMedia && !showVideoPlayer) {
            e.preventDefault();
            window.scrollTo({ top: 0, behavior: "smooth" });
          }
          break;
        case "End":
          if (!selectedMedia && !showVideoPlayer) {
            e.preventDefault();
            window.scrollTo({
              top: document.body.scrollHeight,
              behavior: "smooth",
            });
          }
          break;

      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => {
      window.removeEventListener("keydown", handleKeyDown);
    };
  });

  function openMedia(media) {
    if (!selectedMedia) {
      const scrollContainer = document.getElementById('main-content');
      if (scrollContainer) {
        savedScrollPosition = scrollContainer.scrollTop;
      }
    }
    
    if (selectedMedia?.id !== media.id) {
      historyIndex++;
      // Store clean version in history without autoPlay/resumeProgress
      const cleanMedia = { ...media };
      delete cleanMedia.autoPlay;
      delete cleanMedia.resumeProgress;
      mediaHistory = [...mediaHistory.slice(0, historyIndex), cleanMedia];
    }
    // Set selectedMedia with all flags intact for initial processing
    selectedMedia = media;
  }

  function navigateBack() {
    if (historyIndex > 0) {
      historyIndex--;
      const media = { ...mediaHistory[historyIndex] };
      delete media.autoPlay;
      delete media.resumeProgress;
      selectedMedia = media;
    } else if (historyIndex === 0) {
      historyIndex = -1;
      selectedMedia = null;
      titleBarAccentColor = null;
      requestAnimationFrame(() => {
        const scrollContainer = document.getElementById('main-content');
        if (scrollContainer) {
          scrollContainer.scrollTop = savedScrollPosition;
        }
      });
    } else if (viewAllData) {
      viewAllData = null;
      requestAnimationFrame(() => {
        const scrollContainer = document.getElementById('main-content');
        if (scrollContainer) {
          scrollContainer.scrollTop = savedScrollPosition;
        }
      });
    }
  }

  function navigateForward() {
    if (historyIndex < mediaHistory.length - 1) {
      historyIndex++;
      const media = { ...mediaHistory[historyIndex] };
      delete media.autoPlay;
      delete media.resumeProgress;
      selectedMedia = media;
    }
  }

  function closeDetail() {
    selectedMedia = null;
    titleBarAccentColor = null;
    historyIndex = -1;
    mediaHistory = [];
    requestAnimationFrame(() => {
      const scrollContainer = document.getElementById('main-content');
      if (scrollContainer) {
        scrollContainer.scrollTop = savedScrollPosition;
      }
    });
  }

  // media detail remounts when the player closes, so a spent autoplay request
  // would start the stream again
  function clearAutoPlay() {
    if (!selectedMedia?.autoPlay) return;
    const media = { ...selectedMedia };
    delete media.autoPlay;
    delete media.resumeProgress;
    selectedMedia = media;
  }

  // props stay set on close: the player reads them live, both right after
  // dispatching close (to open the next episode) and in its final save
  function closeVideoPlayer() {
    pipMode = false;
    showVideoPlayer = false;
    clearAutoPlay();
  }

  function backFromVideoPlayer() {
    // Return to media detail that was shown before video player
    pipMode = false;
    showVideoPlayer = false;
    clearAutoPlay();
    // selectedMedia should still be set, so it will show the detail page
  }

  function closeWindow() {
    getCurrentWindow().close();
  }
</script>

<main class:video-active={showVideoPlayer}>
  <Onboarding bind:visible={onboardingVisible} />
  
  {#if activeModal === 'cache'}
    <CacheManager on:close={closeModal} />
  {/if}
  
  {#if activeModal === 'extensions'}
    <ExtensionManager on:close={closeModal} />
  {/if}

  {#if activeModal === 'about'}
    <AboutModal on:close={closeModal} />
  {/if}

  {#if confirmClearHistory}
    <!-- only the recently watched row is cleared; saved progress stays so titles still resume -->
    <ConfirmModal
      title="Clear Recently Watched?"
      message="Every title will be removed from Recently Watched. Your progress is kept, so opening a title again still resumes where you left off."
      confirmLabel="Clear"
      icon="ri-delete-bin-line"
      on:cancel={() => (confirmClearHistory = false)}
      on:confirm={() => {
        confirmClearHistory = false;
        watchHistoryStore.clear();
      }}
    />
  {/if}

  <div class="titlebar-wrapper" class:hidden={showVideoPlayer && (pipMode || !videoControlsVisible)}>
    <TitleBar 
      bind:searchActive 
      bind:settingsActive
      accentColor={showVideoPlayer ? null : titleBarAccentColor} 
      immersive={showVideoPlayer || onboardingVisible}
    />
  </div>
  {#if showVideoPlayer}
    <VideoPlayer {...videoPlayerProps} on:close={closeVideoPlayer} on:back={backFromVideoPlayer} />
  {:else}

    <div class="content-scroll" id="main-content" class:blur={searchActive || settingsActive} use:scrollHoverGuard>
      {#if viewAllData}
        <div style:display={selectedMedia ? 'none' : 'block'} style:pointer-events={selectedMedia ? 'none' : 'auto'}>
          <!-- a new list replaces the page outright, so the old one's items and paging never leak in -->
          {#key viewAllData}
            <ViewAll {...viewAllData} on:close={() => {
              viewAllData = null;
              requestAnimationFrame(() => {
                const scrollContainer = document.getElementById('main-content');
                if (scrollContainer) {
                  scrollContainer.scrollTop = savedScrollPosition;
                }
              });
            }} />
          {/key}
        </div>
      {/if}
      {#if selectedMedia}
        <MediaDetail media={selectedMedia} on:close={navigateBack} />
      {:else if !viewAllData}
        <div class="dashboard">
          {#if !hideRecommendations}
            <RecommendationsCarousel />
          {/if}

          {#if watchHistory.length > 0}
            <MediaCarousel
              title="Recently Watched"
              customItems={watchHistory}
              accentColor="#10b981"
              variant="wide"
              showClearButton={true}
              hideViewAll={true}
              isRecentlyWatched={true}
              watchProgress={$watchProgressStore}
              on:clear={() => (confirmClearHistory = true)}
              on:removeItem={(e) => watchHistoryStore.removeItem(e.detail.id, e.detail.media_type)}
            />
          {/if}

          {#if myList.length > 0}
            <MediaCarousel
              title="My List"
              customItems={myList}
              accentColor="#eab308"
              variant="wide"
              watchProgress={$watchProgressStore}
            />
          {/if}

          {#each HOME_ROWS as row (row.category)}
            <MediaCarousel
              title={row.title}
              category={row.category}
              accentColor={row.accentColor}
              watchProgress={$watchProgressStore}
            />
          {/each}
        </div>
      {/if}
    </div>

    {#if showTorrentDebug}
      <TorrentDebug />
    {/if}
  {/if}
  
  <Updater />

  {#if isLinux}
    <WindowResizeHandles />
  {/if}
</main>

<style>
  @import './styles/app.css';
</style>
