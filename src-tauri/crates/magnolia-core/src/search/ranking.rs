//! scores search results against what the user is about to watch.

use regex::Regex;
use std::sync::OnceLock;

use super::SearchResult;

/// what the results are being ranked for.
#[derive(Debug, Clone, Default)]
pub struct RankContext {
    pub is_movie: bool,
    pub is_tv: bool,
    pub release_year: Option<u32>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
}

/// bytes in a human size like "1.4 GiB". unknown units count as bytes.
pub fn parse_size(size: &str) -> u64 {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"^([\d.]+)\s*(\w+)$").unwrap());
    let Some(c) = re.captures(size.trim()) else { return 0 };
    let Ok(value) = c[1].parse::<f64>() else { return 0 };
    let unit: f64 = match &c[2] {
        "B" => 1.0,
        "KB" | "KiB" => 1024.0,
        "MB" | "MiB" => 1024f64.powi(2),
        "GB" | "GiB" => 1024f64.powi(3),
        "TB" | "TiB" => 1024f64.powi(4),
        _ => 1.0,
    };
    (value * unit) as u64
}

fn season_in_title(title: &str) -> Option<u32> {
    static RES: OnceLock<[Regex; 2]> = OnceLock::new();
    let res = RES.get_or_init(|| {
        [
            Regex::new(r"(?i)S(\d{1,2})").unwrap(),
            Regex::new(r"(?i)SEASON[\s._-]*(\d{1,2})").unwrap(),
        ]
    });
    res.iter().find_map(|re| re.captures(title)?[1].parse().ok())
}

/// whether a result holds the requested episode, directly or as part of a season batch.
pub fn matches_episode(result: &SearchResult, season: u32, episode: u32) -> bool {
    if let (Some(s), Some(e)) = (result.season.filter(|s| *s > 0), result.episode.filter(|e| *e > 0)) {
        return s == season && e == episode;
    }

    let title = result.title.to_uppercase();
    let e = format!("{:02}", episode);
    if title.contains(&format!("S{:02}E{}", season, e)) || title.contains(&format!("{}X{}", season, e)) {
        return true;
    }

    if result.is_batch || title.contains("BATCH") || title.contains("SEASON") {
        if let Some(s) = result.season.filter(|s| *s > 0) {
            return s == season;
        }
        if let Some(s) = season_in_title(&title) {
            return s == season;
        }
    }
    false
}

fn looks_like_batch(title: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)\b(batch|season|complete|s\d{2}|1080p.*(?:season|complete))\b").unwrap())
        .is_match(title)
}

/// fills in the ranking fields of every result. relevance favours well seeded,
/// larger releases and, for shows, whole-season batches.
pub fn rank_results(results: &mut [SearchResult], ctx: &RankContext) {
    let max_seeds = results.iter().map(|r| r.seeds).max().unwrap_or(0) as f64;
    let seed_threshold = max_seeds * 0.1;

    for r in results.iter_mut() {
        r.size_bytes = parse_size(&r.size);
        r.matches_episode = match (ctx.season.filter(|s| *s > 0), ctx.episode.filter(|e| *e > 0)) {
            (Some(s), Some(e)) => matches_episode(r, s, e),
            _ => false,
        };
        r.has_release_year = ctx.is_movie
            && ctx
                .release_year
                .is_some_and(|year| r.title.contains(&year.to_string()));

        let seed_penalty = if (r.seeds as f64) < seed_threshold { 0.5 } else { 1.0 };
        let batch_bonus = if ctx.is_tv && looks_like_batch(&r.title) { 1.5 } else { 1.0 };
        let gib = r.size_bytes as f64 / 1024f64.powi(3);
        r.relevance = ((r.seeds as f64 * 2.0) + r.peers as f64 + gib * 0.1) * seed_penalty * batch_bonus;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(title: &str, seeds: u32, size: &str) -> SearchResult {
        let meta = super::super::parse_title_metadata(title);
        SearchResult {
            title: title.to_string(),
            size: size.to_string(),
            seeds,
            peers: 0,
            magnet_link: String::new(),
            provider: "test".to_string(),
            season: meta.season,
            episode: meta.episode,
            quality: meta.quality,
            encode: meta.encode,
            is_batch: meta.is_batch,
            audio_codec: None,
            ..Default::default()
        }
    }

    #[test]
    fn sizes_parse_with_binary_and_decimal_units() {
        assert_eq!(parse_size("1 KiB"), 1024);
        assert_eq!(parse_size("1.5 GB"), (1.5 * 1024f64.powi(3)) as u64);
        assert_eq!(parse_size("Unknown"), 0);
    }

    #[test]
    fn episode_matching_accepts_exact_and_season_batches() {
        assert!(matches_episode(&result("Show S01E05 1080p", 1, "1 GB"), 1, 5));
        assert!(!matches_episode(&result("Show S01E06 1080p", 1, "1 GB"), 1, 5));
        assert!(matches_episode(&result("Show Season 1 Complete", 1, "1 GB"), 1, 5));
        assert!(!matches_episode(&result("Show Season 2 Complete", 1, "1 GB"), 1, 5));
    }

    #[test]
    fn ranking_penalises_poorly_seeded_results() {
        let mut results = vec![result("Movie 2019 1080p", 100, "2 GB"), result("Movie 1080p", 5, "2 GB")];
        let ctx = RankContext {
            is_movie: true,
            release_year: Some(2019),
            ..Default::default()
        };
        rank_results(&mut results, &ctx);
        assert!(results[0].has_release_year);
        assert!(!results[1].has_release_year);
        assert!(results[0].relevance > results[1].relevance * 10.0);
    }
}
