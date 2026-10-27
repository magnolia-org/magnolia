//! scores search results against what the user is about to watch. every result
//! is scored on its own, so results can be ranked as they stream in.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

use super::release::{normalize_name, release_name, ReleaseInfo, Scope};
use crate::anime_db::AnimeEpisode;
use super::SearchResult;

/// what the results are being ranked for.
#[derive(Debug, Clone, Default)]
pub struct RankContext {
    pub is_movie: bool,
    pub is_anime: bool,
    /// the searched name, normalized with `release::normalize_name`
    pub title: String,
    pub release_year: Option<u32>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    /// other names of the title (romaji, english), normalized; releases may use any of them
    pub alt_titles: Vec<String>,
    /// where the episode falls in anilist's numbering, for anime
    pub anime: Option<AnimeEpisode>,
}

/// how a result relates to the requested movie or episode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchKind {
    #[default]
    Unknown,
    /// the episode itself, or the movie from the right year
    Exact,
    /// a batch that holds the episode
    Pack,
    /// another episode, season or year
    Mismatch,
}

/// bytes in a human size like "1.4 GiB". unknown units count as bytes.
pub fn parse_size(size: &str) -> u64 {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"^([\d.,]+)\s*(\w+)?$").unwrap());
    let Some(c) = re.captures(size.trim()) else { return 0 };
    let Ok(value) = c[1].replace(',', "").parse::<f64>() else { return 0 };
    let unit: f64 = match c.get(2).map_or("B", |m| m.as_str()).to_ascii_uppercase().as_str() {
        "KB" | "KIB" => 1024.0,
        "MB" | "MIB" => 1024f64.powi(2),
        "GB" | "GIB" => 1024f64.powi(3),
        "TB" | "TIB" => 1024f64.powi(4),
        _ => 1.0,
    };
    (value * unit) as u64
}

fn within(value: u32, start: Option<u32>, end: Option<u32>) -> bool {
    match (start, end) {
        (Some(s), Some(e)) => (s..=e).contains(&value),
        (Some(s), None) => s == value,
        _ => false,
    }
}

/// how a release relates to season `season`, episode `episode`.
pub fn episode_match(info: &ReleaseInfo, season: u32, episode: u32) -> MatchKind {
    // releases without a season number are usually numbered from the first season
    let season_matches = |s: Option<u32>| match s {
        Some(s) => Some(s == season),
        None if season == 1 => Some(true),
        None => None,
    };
    match info.scope {
        Scope::Episode => match (season_matches(info.season), info.episode) {
            (Some(true), Some(e)) if e == episode => MatchKind::Exact,
            (None, Some(e)) if e == episode => MatchKind::Unknown,
            (None, _) => MatchKind::Unknown,
            _ => MatchKind::Mismatch,
        },
        Scope::Episodes => match season_matches(info.season) {
            Some(true) if within(episode, info.episode, info.episode_end) => MatchKind::Pack,
            None => MatchKind::Unknown,
            _ => MatchKind::Mismatch,
        },
        Scope::Season => match season_matches(info.season) {
            Some(true) => MatchKind::Pack,
            None => MatchKind::Unknown,
            Some(false) => MatchKind::Mismatch,
        },
        Scope::Seasons if within(season, info.season, info.season_end) => MatchKind::Pack,
        Scope::Seasons => MatchKind::Mismatch,
        Scope::Complete => MatchKind::Pack,
        Scope::Unknown => MatchKind::Unknown,
    }
}

fn match_rank(kind: MatchKind) -> u8 {
    match kind {
        MatchKind::Exact => 3,
        MatchKind::Pack => 2,
        MatchKind::Unknown => 1,
        MatchKind::Mismatch => 0,
    }
}

/// how a release relates to an anime episode in anilist's numbering: the entry's own season
/// and episode ("2nd season - 05"), or the episode counted from the start ("- 33").
pub fn anime_match(info: &ReleaseInfo, anime: &AnimeEpisode, tmdb_season: u32) -> MatchKind {
    // a release without a season number counts from the first entry
    let season_ok = |s: Option<u32>| match s {
        Some(s) => s == anime.season || s == tmdb_season,
        None => anime.season == 1,
    };
    let in_range = |n: u32| within(n, info.episode, info.episode_end);
    match info.scope {
        Scope::Episode => match info.episode {
            Some(e) if e == anime.absolute_episode && info.season.is_none() => MatchKind::Exact,
            Some(e) if e == anime.episode && season_ok(info.season) => MatchKind::Exact,
            _ => MatchKind::Mismatch,
        },
        Scope::Episodes => {
            if (info.season.is_none() && in_range(anime.absolute_episode))
                || (season_ok(info.season) && in_range(anime.episode))
            {
                MatchKind::Pack
            } else {
                MatchKind::Mismatch
            }
        }
        Scope::Season if season_ok(info.season) => MatchKind::Pack,
        Scope::Season if info.season.is_none() => MatchKind::Unknown,
        Scope::Season => MatchKind::Mismatch,
        Scope::Seasons if within(anime.season, info.season, info.season_end) => MatchKind::Pack,
        Scope::Seasons => MatchKind::Mismatch,
        Scope::Complete => MatchKind::Pack,
        Scope::Unknown => MatchKind::Unknown,
    }
}

/// whether a result holds the requested episode, directly or as part of a batch.
pub fn matches_episode(result: &SearchResult, season: u32, episode: u32) -> bool {
    matches!(episode_match(&result.release, season, episode), MatchKind::Exact | MatchKind::Pack)
}

fn movie_match(result: &SearchResult, ctx: &RankContext) -> MatchKind {
    if !matches!(result.release.scope, Scope::Unknown | Scope::Complete) {
        // an episode or season of a show that shares the movie's name
        return MatchKind::Mismatch;
    }
    let Some(year) = ctx.release_year else { return MatchKind::Unknown };
    if result.title.contains(&year.to_string()) {
        return MatchKind::Exact;
    }
    match result.release.year {
        // a year that is part of the name itself ("blade runner 2049") says nothing
        Some(other) if !ctx.title.split(' ').any(|w| w == other.to_string()) => {
            // releases a year off are usually the same film dated by festival or by release
            if other.abs_diff(year) <= 1 {
                MatchKind::Unknown
            } else {
                MatchKind::Mismatch
            }
        }
        _ => MatchKind::Unknown,
    }
}

/// how closely the name at the front of a release matches the searched name, 0 to 1.
pub fn title_score(wanted: &str, title: &str) -> f64 {
    let wanted = normalize_name(wanted);
    let name = release_name(title);
    if wanted.is_empty() || name.is_empty() {
        return 0.5;
    }
    if name == wanted {
        return 1.0;
    }
    if name.starts_with(&format!("{} ", wanted)) {
        return 0.85;
    }
    if format!(" {} ", name).contains(&format!(" {} ", wanted)) {
        return 0.6;
    }
    let words: Vec<&str> = wanted.split(' ').collect();
    let name_words: Vec<&str> = name.split(' ').collect();
    let found = words.iter().filter(|w| name_words.contains(w)).count();
    if found == words.len() {
        0.45
    } else {
        0.4 * found as f64 / words.len() as f64
    }
}

fn resolution_bonus(resolution: Option<&str>) -> f64 {
    match resolution {
        Some("1080p") => 6.0,
        Some("4K") => 5.0,
        Some("1440p") => 3.0,
        Some("720p") => 2.0,
        Some("8K") => 1.0,
        Some("SD") => -6.0,
        _ => 0.0,
    }
}

/// fills in the ranking fields of one result. relevance favours well seeded
/// releases of the right episode or movie, with the right name.
pub fn rank_result(r: &mut SearchResult, ctx: &RankContext) {
    if r.size_bytes == 0 {
        r.size_bytes = parse_size(&r.size);
    }
    r.match_kind = if ctx.is_movie {
        movie_match(r, ctx)
    } else {
        match (ctx.season.filter(|s| *s > 0), ctx.episode.filter(|e| *e > 0)) {
            (Some(s), Some(e)) => {
                let tmdb = episode_match(&r.release, s, e);
                // anime releases follow anilist's numbering, western ones tmdb's; either counts
                match &ctx.anime {
                    Some(anime) => {
                        let by_anime = anime_match(&r.release, anime, s);
                        if match_rank(by_anime) >= match_rank(tmdb) { by_anime } else { tmdb }
                    }
                    None => tmdb,
                }
            }
            _ => MatchKind::Unknown,
        }
    };
    r.title_score = ctx
        .alt_titles
        .iter()
        .map(|t| title_score(t, &r.title))
        .fold(title_score(&ctx.title, &r.title), f64::max);
    // the right episode of what is probably another show ("the bad guys breaking in")
    // can't be called a match. anime names vary too much between romaji and english to tell
    if !ctx.is_anime && r.title_score < 0.5 && matches!(r.match_kind, MatchKind::Exact | MatchKind::Pack) {
        r.match_kind = MatchKind::Unknown;
    }
    r.matches_episode = !ctx.is_movie && matches!(r.match_kind, MatchKind::Exact | MatchKind::Pack);
    r.has_release_year = ctx.is_movie && r.match_kind == MatchKind::Exact;

    let mut score = (1.0 + r.seeds as f64).ln() * 12.0 + (1.0 + r.peers as f64).ln() * 3.0;
    score += match r.match_kind {
        MatchKind::Exact => 40.0,
        MatchKind::Pack if r.release.scope == Scope::Complete => 28.0,
        MatchKind::Pack => 32.0,
        MatchKind::Unknown => 0.0,
        MatchKind::Mismatch => -60.0,
    };
    // anime trackers name shows in romaji, so an english title is a weak signal there
    let title_weight = if ctx.is_anime { 15.0 } else { 45.0 };
    score -= (1.0 - r.title_score) * title_weight;
    if r.release.low_quality {
        score -= 80.0;
    }
    if r.release.upscaled {
        score -= 10.0;
    }
    if r.seeds == 0 {
        score -= 25.0;
    }
    if r.trusted {
        score += 4.0;
    }
    if r.release.repack {
        score += 1.0;
    }
    score += resolution_bonus(r.release.resolution.as_deref());
    r.relevance = score;
}

pub fn rank_results(results: &mut [SearchResult], ctx: &RankContext) {
    for r in results.iter_mut() {
        rank_result(r, ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::super::release::parse_release;
    use super::*;

    fn result(title: &str, seeds: u32, size: &str) -> SearchResult {
        SearchResult {
            title: title.to_string(),
            size: size.to_string(),
            seeds,
            magnet_link: String::new(),
            provider: "test".to_string(),
            release: parse_release(title),
            ..Default::default()
        }
    }

    fn tv(title: &str, season: u32, episode: u32) -> RankContext {
        RankContext {
            title: normalize_name(title),
            season: Some(season),
            episode: Some(episode),
            ..Default::default()
        }
    }

    #[test]
    fn sizes_parse_with_binary_and_decimal_units() {
        assert_eq!(parse_size("1 KiB"), 1024);
        assert_eq!(parse_size("1.5 GB"), (1.5 * 1024f64.powi(3)) as u64);
        assert_eq!(parse_size("2048"), 2048);
        assert_eq!(parse_size("Unknown"), 0);
    }

    #[test]
    fn episode_matching_accepts_exact_and_season_batches() {
        assert!(matches_episode(&result("Show S01E05 1080p", 1, "1 GB"), 1, 5));
        assert!(!matches_episode(&result("Show S01E06 1080p", 1, "1 GB"), 1, 5));
        assert!(matches_episode(&result("Show Season 1 Complete", 1, "1 GB"), 1, 5));
        assert!(!matches_episode(&result("Show Season 2 Complete", 1, "1 GB"), 1, 5));
        assert!(matches_episode(&result("Show S01-S03 1080p", 1, "1 GB"), 2, 5));
        assert!(matches_episode(&result("[Group] Show - 05 (1080p)", 1, "1 GB"), 1, 5));
        assert!(matches_episode(&result("[Group] Show S2 (01-10) (1080p)", 1, "1 GB"), 2, 5));
        assert!(!matches_episode(&result("[Group] Show S2 (01-10) (1080p)", 1, "1 GB"), 2, 11));
    }

    #[test]
    fn titles_score_by_how_closely_the_name_matches() {
        assert_eq!(title_score("Breaking Bad", "Breaking.Bad.S01E05.720p"), 1.0);
        assert!(title_score("Breaking Bad", "The Bad Guys Breaking In S01E05") < 0.5);
        assert!(title_score("Dune", "Dune Part Two 2024 1080p") > 0.8);
    }

    #[test]
    fn ranking_prefers_the_right_episode_over_seeds() {
        let ctx = tv("Breaking Bad", 1, 5);
        let mut exact = result("Breaking Bad S01E05 1080p WEB-DL", 20, "1 GB");
        let mut other = result("Breaking Bad S01E06 1080p WEB-DL", 400, "1 GB");
        let mut wrong_show = result("The Bad Guys Breaking In S01E05 1080p", 50, "1 GB");
        rank_result(&mut exact, &ctx);
        rank_result(&mut other, &ctx);
        rank_result(&mut wrong_show, &ctx);
        assert_eq!(exact.match_kind, MatchKind::Exact);
        assert_eq!(other.match_kind, MatchKind::Mismatch);
        assert!(exact.relevance > other.relevance);
        assert!(exact.relevance > wrong_show.relevance);
        assert_eq!(wrong_show.match_kind, MatchKind::Unknown);
    }

    #[test]
    fn anime_releases_match_in_anilist_numbering() {
        // tmdb season 1 episode 33 is the 2nd season's episode 5
        let ctx = RankContext {
            title: normalize_name("Frieren Beyond Journey's End"),
            alt_titles: vec![normalize_name("Sousou no Frieren")],
            is_anime: true,
            season: Some(1),
            episode: Some(33),
            anime: Some(AnimeEpisode {
                anilist_id: 1,
                title: "Sousou no Frieren 2nd Season".into(),
                franchise_title: "Sousou no Frieren".into(),
                titles: vec![],
                season: 2,
                episode: 5,
                absolute_episode: 33,
                episodes: Some(10),
            }),
            ..Default::default()
        };
        let kind = |title: &str| {
            let mut r = result(title, 10, "1 GB");
            rank_result(&mut r, &ctx);
            (r.match_kind, r.title_score)
        };
        assert_eq!(kind("[SubsPlease] Sousou no Frieren S2 - 05 (1080p)"), (MatchKind::Exact, 1.0));
        assert_eq!(kind("[Erai-raws] Sousou no Frieren 2nd Season - 05 [1080p]").0, MatchKind::Exact);
        assert_eq!(kind("[Group] Sousou no Frieren - 33 (1080p)").0, MatchKind::Exact);
        assert_eq!(kind("[SubsPlease] Sousou no Frieren - 05 (1080p)").0, MatchKind::Mismatch);
        assert_eq!(kind("[SubsPlease] Sousou no Frieren S2 (01-10) (1080p) [Batch]").0, MatchKind::Pack);
        assert_eq!(kind("[Group] Sousou no Frieren (01-28) [Batch]").0, MatchKind::Mismatch);
    }

    #[test]
    fn movies_match_on_release_year() {
        let ctx = RankContext {
            is_movie: true,
            title: normalize_name("Dune"),
            release_year: Some(2021),
            ..Default::default()
        };
        let mut right = result("Dune 2021 1080p BluRay", 100, "2 GB");
        let mut remake = result("Dune 1984 1080p BluRay", 100, "2 GB");
        let mut cam = result("Dune 2021 HDCAM", 300, "2 GB");
        rank_results(std::slice::from_mut(&mut right), &ctx);
        rank_results(std::slice::from_mut(&mut remake), &ctx);
        rank_results(std::slice::from_mut(&mut cam), &ctx);
        assert!(right.has_release_year);
        assert_eq!(remake.match_kind, MatchKind::Mismatch);
        assert!(right.relevance > remake.relevance);
        assert!(right.relevance > cam.relevance);
    }
}
