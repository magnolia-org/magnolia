<script>
  import { createEventDispatcher, onMount, onDestroy } from "svelte";
  import { fade, scale } from "svelte/transition";
  import { cubicOut } from 'svelte/easing';

  export let message = "";
  export let title = "Something went wrong";
  // the raw error, shown folded away for bug reports
  export let details = "";
  // an optional way forward, like picking another source
  export let actionLabel = "";
  export let actionIcon = "ri-arrow-right-line";

  const dispatch = createEventDispatcher();
  let primaryButton;
  let copied = false;
  let copiedTimer;

  onMount(() => primaryButton?.focus());
  onDestroy(() => clearTimeout(copiedTimer));

  function close() {
    dispatch("close");
  }

  function runAction() {
    dispatch("action");
    close();
  }

  async function copyDetails() {
    try {
      await navigator.clipboard.writeText(`${title}\n${message}\n\n${details}`);
      copied = true;
      clearTimeout(copiedTimer);
      copiedTimer = setTimeout(() => (copied = false), 1500);
    } catch (err) {
      console.error("failed to copy error details:", err);
    }
  }

  function handleKeydown(event) {
    if (event.key === "Escape") {
      event.stopPropagation();
      close();
    }
  }
</script>

<svelte:window on:keydown={handleKeydown} />

<!-- svelte-ignore a11y-click-events-have-key-events -->
<!-- svelte-ignore a11y-no-static-element-interactions -->
<div class="alert-overlay" transition:fade|global={{ duration: 400, easing: cubicOut }} on:click={close}>
  <!-- svelte-ignore a11y-click-events-have-key-events -->
  <!-- svelte-ignore a11y-no-static-element-interactions -->
  <div class="alert-card" role="alertdialog" tabindex="-1" aria-labelledby="error-title" aria-describedby="error-message" on:click|stopPropagation in:scale|global={{ start: 1.08, opacity: 1, duration: 400, easing: cubicOut }} out:scale|global={{ start: 1.08, opacity: 1, duration: 400, easing: cubicOut }}>
    <div class="alert-icon"><i class="ri-error-warning-line"></i></div>
    <h3 class="alert-title" id="error-title">{title}</h3>
    <p class="alert-message" id="error-message">{message}</p>

    {#if details}
      <details class="alert-details">
        <summary>
          <i class="ri-arrow-right-s-line"></i>
          Technical details
        </summary>
        <div class="alert-details-body">
          <pre>{details}</pre>
          <button class="alert-copy" on:click={copyDetails} title="Copy for a bug report">
            <i class={copied ? "ri-check-line" : "ri-file-copy-line"}></i>
            {copied ? "Copied" : "Copy"}
          </button>
        </div>
      </details>
    {/if}

    <div class="alert-actions">
      {#if actionLabel}
        <button class="btn-standard" on:click={close}>Dismiss</button>
        <button class="btn-standard alert-primary" bind:this={primaryButton} on:click={runAction}>
          <i class={actionIcon}></i>
          {actionLabel}
        </button>
      {:else}
        <button class="btn-standard alert-primary" bind:this={primaryButton} on:click={close}>OK</button>
      {/if}
    </div>
  </div>
</div>

<style>
  @import '../styles/error-modal.css';

  .alert-details {
    width: 100%;
    margin-top: var(--spacing-lg);
    font-size: 13px;
  }

  .alert-details summary {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    cursor: pointer;
    color: var(--text-tertiary);
    list-style: none;
    user-select: none;
    transition: color 0.2s ease;
  }

  .alert-details summary::-webkit-details-marker {
    display: none;
  }

  .alert-details summary:hover {
    color: var(--text-secondary);
  }

  .alert-details summary i {
    font-size: 16px;
    transition: transform 0.2s ease;
  }

  .alert-details[open] summary i {
    transform: rotate(90deg);
  }

  .alert-details-body {
    position: relative;
    margin-top: var(--spacing-sm);
  }

  .alert-details pre {
    margin: 0;
    max-height: 160px;
    overflow: auto;
    padding: var(--spacing-md) 72px var(--spacing-md) var(--spacing-md);
    font-family: 'Geist Mono Variable', monospace;
    font-size: 12px;
    line-height: 1.5;
    color: var(--text-secondary);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    background: rgba(0, 0, 0, 0.35);
    border: 1px solid rgba(255, 255, 255, 0.06);
    border-radius: var(--border-radius-md);
    user-select: text;
  }

  .alert-copy {
    position: absolute;
    top: 8px;
    right: 8px;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 4px 8px;
    font: inherit;
    font-size: 11px;
    color: var(--text-secondary);
    background: rgba(255, 255, 255, 0.08);
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: var(--border-radius-sm);
    cursor: pointer;
    transition: background 0.2s ease, color 0.2s ease;
  }

  .alert-copy:hover {
    background: rgba(255, 255, 255, 0.14);
    color: var(--text-primary);
  }

  .alert-primary {
    background: rgba(255, 255, 255, 0.9);
    border-color: transparent;
    color: #0a0a0a;
    font-weight: 600;
  }

  .alert-primary:hover {
    background: #fff;
    color: #000;
  }
</style>
