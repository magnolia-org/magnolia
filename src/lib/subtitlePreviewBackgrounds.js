// Pool of background images used behind the subtitle settings preview.
//
// To add another image: drop the file in `src/media/subtitle-previews/` and add
// an entry below. `src` is a static import (Vite hashes/bundles it for you) and
// `attribution` is the small credit line rendered under the preview (HTML allowed).
import magnolia from "../media/subtitle-previews/magnolia.jpg";

export const subtitlePreviewBackgrounds = [
  {
    src: magnolia,
    attribution:
      'Photo by <a href="https://unsplash.com/@tlmn?utm_source=unsplash&utm_medium=referral&utm_content=creditCopyText">Til Man</a> on <a href="https://unsplash.com/photos/pink-and-white-flowers-during-daytime-5oAJ5KeZxNI?utm_source=unsplash&utm_medium=referral&utm_content=creditCopyText">Unsplash</a>',
  },
];

// Pick a random image from the pool.
export function randomSubtitlePreviewBackground() {
  return subtitlePreviewBackgrounds[
    Math.floor(Math.random() * subtitlePreviewBackgrounds.length)
  ];
}
