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
