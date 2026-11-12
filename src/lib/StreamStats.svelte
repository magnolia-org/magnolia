<script>
  // one entry per second, oldest first: { down, up } in bytes per second
  export let samples = [];
  export let stats = null;
  export let bufferAhead = 0;
  // how many seconds the graph spans
  export let span = 120;

  const WIDTH = 320;
  const HEIGHT = 96;
  const TOP = 16;
  const UNITS = ["B", "KB", "MB", "GB", "TB"];

  let hoverIndex = null;

  function formatBytes(bytes) {
    if (!bytes || bytes < 1) return "0 B";
    const unit = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), UNITS.length - 1);
    const value = bytes / Math.pow(1024, unit);
    return `${value >= 100 || unit === 0 ? value.toFixed(0) : value.toFixed(1)} ${UNITS[unit]}`;
  }

  const formatSpeed = (bytes) => `${formatBytes(bytes)}/s`;

  // the axis tops out at a round number above the fastest sample
  function niceMax(value) {
    const floor = 256 * 1024;
    if (value <= floor) return floor;
    const magnitude = Math.pow(2, Math.floor(Math.log2(value)));
    return [1, 1.5, 2].map((step) => step * magnitude).find((top) => top >= value);
  }

  $: shown = samples.slice(-span);
  $: peak = shown.reduce((max, s) => Math.max(max, s.down), 0);
  $: average = shown.length ? shown.reduce((sum, s) => sum + s.down, 0) / shown.length : 0;
  $: top = niceMax(shown.reduce((max, s) => Math.max(max, s.down, s.up), 0));
  $: xOf = (i) => WIDTH - (shown.length - 1 - i) * (WIDTH / (span - 1));
  $: yOf = (value) => HEIGHT - (value / top) * (HEIGHT - TOP);
  $: downLine = shown.map((s, i) => `${i ? "L" : "M"}${xOf(i).toFixed(1)},${yOf(s.down).toFixed(1)}`).join(" ");
  $: upLine = shown.map((s, i) => `${i ? "L" : "M"}${xOf(i).toFixed(1)},${yOf(s.up).toFixed(1)}`).join(" ");
  $: downArea = shown.length > 1 ? `${downLine} L${WIDTH},${HEIGHT} L${xOf(0).toFixed(1)},${HEIGHT} Z` : "";
  $: hovered = hoverIndex !== null && shown[hoverIndex] ? shown[hoverIndex] : null;
  $: current = hovered || shown[shown.length - 1] || { down: 0, up: 0 };
  $: filePercent = stats?.file_size ? (stats.file_bytes / stats.file_size) * 100 : 0;

  function handleHover(event) {
    if (shown.length === 0) return;
    const rect = event.currentTarget.getBoundingClientRect();
    const x = ((event.clientX - rect.left) / rect.width) * WIDTH;
    const fromRight = Math.round((WIDTH - x) / (WIDTH / (span - 1)));
    const index = shown.length - 1 - fromRight;
    hoverIndex = index >= 0 && index < shown.length ? index : null;
  }
</script>

<div class="stats-page">
  {#if !stats}
    <div class="stats-empty">
      <span class="loading-spinner-small"></span>
      Waiting for the torrent...
    </div>
  {:else}
    <div class="stats-speeds">
      <div class="stats-speed down">
        <span class="stats-speed-label"><i class="ri-arrow-down-line"></i> Download</span>
        <span class="stats-speed-value">{formatSpeed(current.down)}</span>
      </div>
      <div class="stats-speed up">
        <span class="stats-speed-label"><i class="ri-arrow-up-line"></i> Upload</span>
        <span class="stats-speed-value">{formatSpeed(current.up)}</span>
      </div>
    </div>

    <div class="stats-chart-card">
    <!-- svelte-ignore a11y-no-static-element-interactions -->
    <div class="stats-chart" on:mousemove={handleHover} on:mouseleave={() => (hoverIndex = null)}>
      <svg viewBox="0 0 {WIDTH} {HEIGHT}" preserveAspectRatio="none" role="img" aria-label="Download and upload speed over the last {span} seconds">
        <defs>
          <linearGradient id="stats-down-fill" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stop-color="var(--accent-color)" stop-opacity="0.35" />
            <stop offset="100%" stop-color="var(--accent-color)" stop-opacity="0" />
          </linearGradient>
        </defs>
        <line class="stats-grid" x1="0" x2={WIDTH} y1={TOP} y2={TOP} />
        <line class="stats-grid" x1="0" x2={WIDTH} y1={(TOP + HEIGHT) / 2} y2={(TOP + HEIGHT) / 2} />
        <line class="stats-baseline" x1="0" x2={WIDTH} y1={HEIGHT - 0.5} y2={HEIGHT - 0.5} />
        {#if downArea}
          <path class="stats-down-area" d={downArea} />
        {/if}
        {#if shown.length > 1}
          <path class="stats-up-line" d={upLine} />
          <path class="stats-down-line" d={downLine} />
        {/if}
        {#if hovered}
          <line class="stats-crosshair" x1={xOf(hoverIndex)} x2={xOf(hoverIndex)} y1={TOP} y2={HEIGHT} />
        {/if}
      </svg>
      {#if hovered}
        <span class="stats-dot down" style="left: {(xOf(hoverIndex) / WIDTH) * 100}%; top: {(yOf(hovered.down) / HEIGHT) * 100}%"></span>
        <span class="stats-dot up" style="left: {(xOf(hoverIndex) / WIDTH) * 100}%; top: {(yOf(hovered.up) / HEIGHT) * 100}%"></span>
      {/if}
      <span class="stats-axis-top">{formatSpeed(top)}</span>
    </div>
    <div class="stats-axis-time">
      <span>{span / 60} min ago</span>
      <span>{hovered ? `${shown.length - 1 - hoverIndex}s ago` : "now"}</span>
    </div>
    </div>

    <div class="stats-file">
      <div class="stats-file-row">
        <span class="stats-label">File downloaded</span>
        <span class="stats-file-percent">{filePercent.toFixed(0)}%</span>
      </div>
      <div class="stats-file-track">
        <div class="stats-file-fill" style="width: {filePercent}%"></div>
      </div>
      <span class="stats-file-value">{formatBytes(stats.file_bytes)} of {formatBytes(stats.file_size)}</span>
    </div>

    <div class="stats-grid-tiles">
      {#each [
        { icon: "ri-download-2-line", label: "Downloaded", value: formatBytes(stats.downloaded_bytes) },
        { icon: "ri-upload-2-line", label: "Uploaded", value: formatBytes(stats.uploaded_bytes) },
        { icon: "ri-group-line", label: "Peers", value: stats.peers, sub: `/ ${stats.peers_seen} seen` },
        { icon: "ri-timer-flash-line", label: "Buffered ahead", value: bufferAhead >= 60 ? `${(bufferAhead / 60).toFixed(1)} min` : `${Math.round(bufferAhead)}s` },
        { icon: "ri-flashlight-line", label: "Peak speed", value: formatSpeed(peak) },
        { icon: "ri-pulse-line", label: "Average speed", value: formatSpeed(average) },
      ] as tile}
        <div class="stats-tile">
          <i class="stats-tile-icon {tile.icon}"></i>
          <div class="stats-tile-text">
            <span class="stats-label">{tile.label}</span>
            <span class="stats-value">{tile.value}{#if tile.sub}<span class="stats-sub"> {tile.sub}</span>{/if}</span>
          </div>
        </div>
      {/each}
    </div>

    <div class="stats-footnote">
      <span class="stats-chip">Piece {formatBytes(stats.piece_size)}</span>
      {#if stats.peers_connecting}
        <span class="stats-chip">{stats.peers_connecting} connecting</span>
      {/if}
    </div>
  {/if}
</div>
