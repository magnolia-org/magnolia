<script>
import { onMount, onDestroy, afterUpdate } from 'svelte';

// horizontal row with arrow buttons and edges that fade out while there is more to scroll to
export let gap = 'var(--spacing-lg)';
export let className = '';

let track;
let canScrollLeft = false;
let canScrollRight = false;
let frame = null;
let resizeObserver;

function update() {
  frame = null;
  if (!track) return;
  canScrollLeft = track.scrollLeft > 10;
  canScrollRight = track.scrollLeft < track.scrollWidth - track.clientWidth - 10;
}

function scheduleUpdate() {
  if (frame === null) frame = requestAnimationFrame(update);
}

// pages by most of the visible width so the next set of cards lands in view
function scroll(direction) {
  if (!track) return;
  const amount = Math.max(track.clientWidth * 0.8, 300);
  track.scrollBy({ left: direction === 'left' ? -amount : amount, behavior: 'smooth' });
}

onMount(() => {
  resizeObserver = new ResizeObserver(scheduleUpdate);
  resizeObserver.observe(track);
  scheduleUpdate();
});

afterUpdate(scheduleUpdate);

onDestroy(() => {
  resizeObserver?.disconnect();
  if (frame !== null) cancelAnimationFrame(frame);
});
</script>

<div class="scroller {className}" class:fade-left={canScrollLeft} class:fade-right={canScrollRight}>
  <button class="carousel-arrow left" class:visible={canScrollLeft} on:click={() => scroll('left')} aria-label="Scroll left" tabindex="-1">
    <i class="ri-arrow-left-s-line"></i>
  </button>
  <div class="scroller-track" style:gap bind:this={track} on:scroll={scheduleUpdate}>
    <slot />
  </div>
  <button class="carousel-arrow right" class:visible={canScrollRight} on:click={() => scroll('right')} aria-label="Scroll right" tabindex="-1">
    <i class="ri-arrow-right-s-line"></i>
  </button>
</div>
