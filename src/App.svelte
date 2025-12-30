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
  import { myListStore } from "./lib/stores/listStore.js";
  import { watchHistoryStore } from "./lib/stores/watchHistoryStore.js";
  import { watchProgressStore } from "./lib/stores/watchProgressStore.js";
  import { getCurrentWindow } from "@tauri-apps/api/window";

  let searchActive = false;
  let settingsActive = false;
  let selectedMedia = null;
  let viewAllData = null;
  let titleBarAccentColor = null;
  let mediaHistory = [];
  let historyIndex = -1;
  let showTorrentDebug = false;
  let savedScrollPosition = 0;

  // Video Player State
  let showVideoPlayer = false;
  let videoPlayerProps = null;
  let videoControlsVisible = true;
  let onboardingVisible = false;

  $: myList = $myListStore;
  $: watchHistory = $watchHistoryStore;
  $: watchProgress = $watchProgressStore;

  onMount(() => {
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
      viewAllData = e.detail;
    });

    window.addEventListener("openVideoPlayer", (e) => {
      console.log("Opening video player with:", e.detail);
      
      // If video player is already open, close it first to force remount
      if (showVideoPlayer) {
        showVideoPlayer = false;
        videoPlayerProps = null;
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
          if (viewAllData) {
            e.preventDefault();
            viewAllData = null;
          } else if (selectedMedia) {
            e.preventDefault();
            closeDetail();
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

  function closeVideoPlayer() {
    showVideoPlayer = false;
    videoPlayerProps = null;
  }

  function backFromVideoPlayer() {
    // Return to media detail that was shown before video player
    showVideoPlayer = false;
    videoPlayerProps = null;
    // selectedMedia should still be set, so it will show the detail page
  }

  function closeWindow() {
    getCurrentWindow().close();
  }
</script>

<main>
  <Onboarding bind:visible={onboardingVisible} />
  <div class="titlebar-wrapper" class:hidden={showVideoPlayer && !videoControlsVisible}>
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

    <div class="content-scroll" id="main-content" class:blur={searchActive || settingsActive}>
      {#if selectedMedia}
        <MediaDetail media={selectedMedia} on:close={navigateBack} />
      {:else if viewAllData}
        <ViewAll {...viewAllData} on:close={() => { 
          viewAllData = null; 
          requestAnimationFrame(() => {
            const scrollContainer = document.getElementById('main-content');
            if (scrollContainer) {
              scrollContainer.scrollTop = savedScrollPosition;
            }
          });
        }} />
      {:else}
        <div class="dashboard">
          <RecommendationsCarousel />

          {#if watchHistory.length > 0}
            <MediaCarousel
              title="Recently Watched"
              customItems={watchHistory}
              accentColor="#10b981"
              showClearButton={true}
              hideViewAll={true}
              isRecentlyWatched={true}
              watchProgress={$watchProgressStore}
              on:clear={() => watchHistoryStore.clear()}
              on:removeItem={(e) => {
                watchHistoryStore.removeItem(e.detail.id, e.detail.media_type);
                watchProgressStore.removeProgress(e.detail.id, e.detail.media_type);
              }}
            />
          {/if}

          {#if myList.length > 0}
            <MediaCarousel
              title="My List"
              customItems={myList}
              accentColor="#eab308"
            />
          {/if}

          <MediaCarousel
            title="Trending Movies"
            type="movie"
            category="trending"
            accentColor="#f43f5e"
          />

          <MediaCarousel
            title="Popular Movies"
            type="movie"
            category="popular"
            accentColor="#ec4899"
          />

          <MediaCarousel
            title="Top Rated Movies"
            type="movie"
            category="top_rated"
            accentColor="#8b5cf6"
          />

          <MediaCarousel
            title="Trending TV Shows"
            type="tv"
            category="trending"
            accentColor="#3b82f6"
          />

          <MediaCarousel
            title="Popular TV Shows"
            type="tv"
            category="popular"
            accentColor="#06b6d4"
          />
        </div>
      {/if}
    </div>

    {#if showTorrentDebug}
      <TorrentDebug />
    {/if}
  {/if}
</main>

<style>
  @import './styles/app.css';
</style>
