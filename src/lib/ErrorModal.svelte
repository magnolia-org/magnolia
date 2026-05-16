<script>
  import { createEventDispatcher } from "svelte";
  import { fade, scale } from "svelte/transition";
  import { sineOut } from 'svelte/easing';

  export let message = "";
  export let title = "Error";

  const dispatch = createEventDispatcher();

  function close() {
    dispatch("close");
  }

  function handleKeydown(event) {
    if (event.key === "Escape") {
      close();
    }
  }
</script>

<svelte:window on:keydown={handleKeydown} />

<!-- svelte-ignore a11y-click-events-have-key-events -->
<!-- svelte-ignore a11y-no-static-element-interactions -->
<div class="modal-overlay" transition:fade on:click={close}>
  <!-- svelte-ignore a11y-click-events-have-key-events -->
  <!-- svelte-ignore a11y-no-static-element-interactions -->
  <div class="modal-content" in:scale={{ duration: 180, start: 0.94 }} out:scale={{ duration: 220, start: 0.8, easing: sineOut }} on:click|stopPropagation>
    <div class="modal-header">
      <i class="ri-error-warning-line error-icon"></i>
      <h3>{title}</h3>
    </div>
    <div class="modal-body">
      <p>{message}</p>
    </div>
    <div class="modal-footer">
      <button class="btn-standard" on:click={close}>
        OK
      </button>
    </div>
  </div>
</div>

<style>
  @import '../styles/error-modal.css';
</style>
