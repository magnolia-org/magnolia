// Piecewise linear color stops: [value, r, g, b]
// Each rating milestone eases smoothly into the next.
const RATING_STOPS = [
  [0,  214, 93,  177],  // #d65db1 purple
  [5,  255, 107, 107],  // #ff6b6b red
  [6,  255, 163, 104],  // #ffa368 orange
  [7,  245, 217, 90 ],  // #f5d95a yellow
  [8,  107, 219, 143],  // #6bdb8f green
  [9,  95,  237, 216],  // #5fedd8 aqua
  [10, 95,  237, 216],  // #5fedd8 aqua (clamp)
];

/**
 * Returns an interpolated rgb() color for any rating 0–10.
 * Colors ease smoothly between milestones rather than snapping.
 */
export function getRatingColor(rating) {
  const r = Math.max(0, Math.min(10, rating || 0));
  for (let i = 0; i < RATING_STOPS.length - 1; i++) {
    const [v0, r0, g0, b0] = RATING_STOPS[i];
    const [v1, r1, g1, b1] = RATING_STOPS[i + 1];
    if (r >= v0 && r <= v1) {
      const t = (v1 - v0) === 0 ? 0 : (r - v0) / (v1 - v0);
      return `rgb(${Math.round(r0 + t * (r1 - r0))}, ${Math.round(g0 + t * (g1 - g0))}, ${Math.round(b0 + t * (b1 - b0))})`;
    }
  }
  return 'rgb(95, 237, 216)';
}

/**
 * Turns a colour sampled from artwork into a tile accent. It keeps the art's own hue and
 * roughly its saturation, so soft art gets a soft accent, and fixes the lightness so a dark
 * icon on top always reads.
 */
export function artAccent(r, g, b) {
  const max = Math.max(r, g, b) / 255;
  const min = Math.min(r, g, b) / 255;
  const delta = max - min;
  const light = (max + min) / 2;
  const sat = delta === 0 ? 0 : delta / (1 - Math.abs(2 * light - 1));

  let hue = 0;
  if (delta > 0) {
    if (max === r / 255) hue = ((g - b) / 255 / delta) % 6;
    else if (max === g / 255) hue = (b - r) / 255 / delta + 2;
    else hue = (r - g) / 255 / delta + 4;
  }
  hue = Math.round(((hue * 60) + 360) % 360);

  return `hsl(${hue}, ${Math.round(Math.min(Math.max(sat * 1.3, 0.3), 0.8) * 100)}%, 68%)`;
}

// resolves once the image has loaded. img.decode() would be simpler, but chromium holds decodes
// while the window is hidden, which would leave tiles uncoloured until it is shown again
export function loadImage(url) {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.crossOrigin = 'Anonymous';
    img.onload = () => resolve(img);
    img.onerror = () => reject(new Error(`failed to load ${url}`));
    img.src = url;
    // webkit may not fire onload again for a cached image
    if (img.complete && img.naturalWidth > 0) resolve(img);
  });
}

const accentCache = new Map();

/**
 * The accent for a piece of art. Art with a strong colour cluster (a blue sky, amber desert)
 * gets that colour. Soft, mostly pale art gets its overall tint instead, since that is what
 * reads as its colour (NieR's greys come out lavender, not the small blue patches). Only
 * art with no tint at all (black and white, pure greys) gets the fallback.
 */
export function extractArtAccent(imageUrl, fallback) {
  if (!accentCache.has(imageUrl)) {
    accentCache.set(imageUrl, (async () => {
      const img = await loadImage(imageUrl);

      const canvas = document.createElement('canvas');
      canvas.width = img.naturalWidth;
      canvas.height = img.naturalHeight;
      const ctx = canvas.getContext('2d', { willReadFrequently: true });
      ctx.drawImage(img, 0, 0);
      const data = ctx.getImageData(0, 0, canvas.width, canvas.height).data;

      // 24 hue buckets of 15 degrees each
      const buckets = Array.from({ length: 24 }, () => ({ r: 0, g: 0, b: 0, weight: 0 }));
      let samples = 0;
      const overall = { r: 0, g: 0, b: 0, count: 0 };
      for (let i = 0; i < data.length; i += 16) {
        const red = data[i], green = data[i + 1], blue = data[i + 2];
        const max = Math.max(red, green, blue);
        const min = Math.min(red, green, blue);
        samples++;
        // near-black and blown-out pixels carry no real colour
        if (max < 40 || min > 245) continue;
        overall.r += red;
        overall.g += green;
        overall.b += blue;
        overall.count++;
        const chroma = max - min;
        if (chroma < 10) continue;

        let hue;
        if (max === red) hue = ((green - blue) / chroma) % 6;
        else if (max === green) hue = (blue - red) / chroma + 2;
        else hue = (red - green) / chroma + 4;
        const bucket = buckets[Math.floor((((hue * 60) + 360) % 360) / 15)];
        const weight = chroma;
        bucket.r += red * weight;
        bucket.g += green * weight;
        bucket.b += blue * weight;
        bucket.weight += weight;
      }

      // neighbouring buckets count toward each other so a hue split across a boundary still wins
      let best = null;
      let bestScore = 0;
      buckets.forEach((bucket, i) => {
        const score = bucket.weight
          + 0.5 * buckets[(i + 1) % 24].weight
          + 0.5 * buckets[(i + 23) % 24].weight;
        if (score > bestScore) {
          bestScore = score;
          best = bucket;
        }
      });

      // measured on real posters and backdrops: bold art scores 10 or more, soft pale art about 4
      if (best && bestScore / samples >= 8) {
        return artAccent(best.r / best.weight, best.g / best.weight, best.b / best.weight);
      }
      if (overall.count === 0) return null;
      const r = overall.r / overall.count;
      const g = overall.g / overall.count;
      const b = overall.b / overall.count;
      const max = Math.max(r, g, b);
      const min = Math.min(r, g, b);
      const light = (max + min) / 2 / 255;
      const sat = max === min ? 0 : (max - min) / 255 / (1 - Math.abs(2 * light - 1));
      return sat < 0.025 ? null : artAccent(r, g, b);
    })().catch(() => {
      accentCache.delete(imageUrl);
      return undefined;
    }));
  }
  // the cache holds null for untinted art, so each caller's own fallback applies; undefined
  // means the load failed and stays undefined so the caller can retry later
  return accentCache.get(imageUrl).then((color) => (color === null ? fallback : color));
}
