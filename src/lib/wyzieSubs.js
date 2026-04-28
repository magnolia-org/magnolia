import { configure, searchSubtitles } from "wyzie-lib";

export async function fetchSubtitles(mediaId, mediaType, seasonNum = null, episodeNum = null) {
  configure({
    baseUrl: "https://wyzie.wyziemagnolia.workers.dev"
  });
  try {
    const params = {
      tmdb_id: mediaId,
      format: "srt",
    };

    if (mediaType === "tv" && seasonNum !== null && episodeNum !== null) {
      params.season = seasonNum;
      params.episode = episodeNum;
    }

    console.log("fetching Wyzie subtitles with params:", params);
    const subtitles = await searchSubtitles(params);
    console.log("Wyzie subtitles found:", subtitles.length);
    
    // Deduplicate by language and hearing impaired status
    const seen = new Set();
    const uniqueSubs = subtitles.filter(sub => {
      const key = `${sub.language}-${sub.isHearingImpaired}`;
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    });
    
    console.log("unique Wyzie subtitles after deduplication:", uniqueSubs.length);
    
    return uniqueSubs.map(sub => ({
      id: sub.id,
      language: sub.language,
      display: sub.display,
      url: sub.url,
      format: sub.format,
      isHearingImpaired: sub.isHearingImpaired,
      encoding: sub.encoding,
      source: "wyzie",
      name: `${sub.display}${sub.isHearingImpaired ? " (HI)" : ""}`
    }));
  } catch (error) {
    console.error("failed to fetch Wyzie subtitles:", error);
    return [];
  }
}

export async function downloadSubtitle(url) {
  try {
    const response = await fetch(url);
    if (!response.ok) {
      throw new Error(`Failed to download subtitle: ${response.statusText}`);
    }
    const text = await response.text();
    return text;
  } catch (error) {
    console.error("failed to download subtitle:", error);
    throw error;
  }
}
