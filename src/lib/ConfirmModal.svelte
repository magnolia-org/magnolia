<script>
  import { createEventDispatcher, onMount } from "svelte";
  import { fade, scale } from "svelte/transition";
  import { cubicOut } from 'svelte/easing';

  export let title = "Are you sure?";
  export let message = "";
  export let confirmLabel = "Confirm";
  export let icon = "ri-error-warning-line";

  const dispatch = createEventDispatcher();
  let cancelButton;

  // focus lands on cancel so a stray enter never confirms
  onMount(() => cancelButton?.focus());

  function cancel() {
    dispatch("cancel");
  }

  function handleKeydown(event) {
    if (event.key === "Escape") {
      event.stopPropagation();
      cancel();
    }
  }
</script>

<svelte:window on:keydown={handleKeydown} />

<!-- svelte-ignore a11y-click-events-have-key-events -->
<!-- svelte-ignore a11y-no-static-element-interactions -->
<div class="alert-overlay" transition:fade|global={{ duration: 400, easing: cubicOut }} on:click={cancel}>
  <!-- svelte-ignore a11y-click-events-have-key-events -->
  <!-- svelte-ignore a11y-no-static-element-interactions -->
  <div class="alert-card" role="alertdialog" tabindex="-1" aria-labelledby="confirm-title" aria-describedby="confirm-message" on:click|stopPropagation in:scale|global={{ start: 1.08, opacity: 1, duration: 400, easing: cubicOut }} out:scale|global={{ start: 1.08, opacity: 1, duration: 400, easing: cubicOut }}>
    <div class="alert-icon"><i class={icon}></i></div>
    <h3 class="alert-title" id="confirm-title">{title}</h3>
    <p class="alert-message" id="confirm-message">{message}</p>
    <div class="alert-actions">
      <button class="btn-standard" bind:this={cancelButton} on:click={cancel}>Cancel</button>
      <button class="btn-standard alert-danger" on:click={() => dispatch("confirm")}>{confirmLabel}</button>
    </div>
  </div>
</div>

<style>
  @import '../styles/error-modal.css';

  .alert-danger {
    background: color-mix(in srgb, var(--alert-tone) 20%, transparent);
    border-color: color-mix(in srgb, var(--alert-tone) 55%, transparent);
    color: #fca5a5;
    font-weight: 600;
  }

  .alert-danger:hover {
    background: color-mix(in srgb, var(--alert-tone) 32%, transparent);
    border-color: color-mix(in srgb, var(--alert-tone) 80%, transparent);
    color: #fff;
  }
</style>
