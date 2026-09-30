//! picking the right video file out of a torrent.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

const VIDEO_EXTENSIONS: &[&str] = &[".mkv", ".mp4", ".avi", ".mov", ".webm", ".m4v"];

pub fn is_video_file(name: &str) -> bool {
    let lower = name.to_lowercase();
    VIDEO_EXTENSIONS.iter().any(|ext| lower.ends_with(ext))
}

/// a playable file inside a torrent, from librqbit or a debrid service.
/// `index` is what the streaming side wants back when this file is played.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaFile {
    pub index: usize,
    pub name: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub path: String,
}

/// position in `files` of the file holding the requested episode.
pub fn find_episode_file(files: &[MediaFile], season: u32, episode: u32) -> Option<usize> {
    let s = format!("{:02}", season);
    let e = format!("{:02}", episode);
    let exact = [format!("S{}E{}", s, e), format!("{}X{}", season, e)];
    let upper: Vec<String> = files.iter().map(|f| f.name.to_uppercase()).collect();

    if let Some(i) = upper.iter().position(|n| exact.iter().any(|p| n.contains(p.as_str()))) {
        return Some(i);
    }
    // a single-file torrent can only mean one thing
    if files.len() == 1 {
        return Some(0);
    }
    // season packs often only number the episode
    let loose = [format!("E{}", e), format!(" {} ", e)];
    upper.iter().position(|n| loose.iter().any(|p| n.contains(p.as_str())))
}

struct EpisodeRegexes {
    season_episode: Regex,
    cross: Regex,
    episode_word: Regex,
    delimited: Regex,
}

fn episode_regexes() -> &'static EpisodeRegexes {
    static REGEXES: OnceLock<EpisodeRegexes> = OnceLock::new();
    REGEXES.get_or_init(|| EpisodeRegexes {
        // S01E05, s1e5
        season_episode: Regex::new(r"(?i)S(\d{1,2})E(\d{1,3})").unwrap(),
        // 1x05 (bounded so resolutions like 1920x1080 don't match)
        cross: Regex::new(r"(?i)\b(\d{1,2})x(\d{2,3})\b").unwrap(),
        // Episode 5, Ep 05, E05 (a bare E must start a word)
        episode_word: Regex::new(r"(?i)(?:Episode|Ep\.?|(?:^|[^0-9a-zA-Z])E)[\s._-]*(\d{1,3})").unwrap(),
        // "- 05 -" / "[05]", common in anime releases
        delimited: Regex::new(r"[-\[\s](\d{2,3})[-\]\s]").unwrap(),
    })
}

/// (season, episode) numbered in a file name. files that only number the
/// episode are assumed to belong to `default_season`.
pub fn parse_episode_number(filename: &str, default_season: u32) -> Option<(u32, u32)> {
    let r = episode_regexes();
    let parsed = if let Some(c) = r.season_episode.captures(filename) {
        Some((c[1].parse().ok()?, c[2].parse().ok()?))
    } else if let Some(c) = r.cross.captures(filename) {
        Some((c[1].parse().ok()?, c[2].parse().ok()?))
    } else if let Some(c) = r.episode_word.captures(filename) {
        Some((default_season, c[1].parse().ok()?))
    } else if let Some(c) = r.delimited.captures(filename) {
        Some((default_season, c[1].parse().ok()?))
    } else {
        None
    };
    parsed.filter(|(_, e)| *e > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(index: usize, name: &str) -> MediaFile {
        MediaFile {
            index,
            name: name.to_string(),
            size: 0,
            path: name.to_string(),
        }
    }

    #[test]
    fn video_extensions() {
        assert!(is_video_file("Show.S01E01.MKV"));
        assert!(is_video_file("clip.webm"));
        assert!(!is_video_file("Show.S01E01.srt"));
    }

    #[test]
    fn finds_exact_then_loose_episode() {
        let files = vec![file(0, "Show.S01E01.mkv"), file(1, "Show.S01E02.mkv")];
        assert_eq!(find_episode_file(&files, 1, 2), Some(1));
        assert_eq!(find_episode_file(&files, 1, 3), None);

        let files = vec![file(4, "[Group] Show - 01 [1080p].mkv"), file(5, "[Group] Show - 02 [1080p].mkv")];
        assert_eq!(find_episode_file(&files, 1, 2), Some(1));

        let single = vec![file(7, "Movie.2019.1080p.mkv")];
        assert_eq!(find_episode_file(&single, 0, 0), Some(0));
    }

    #[test]
    fn parses_episode_numbers() {
        assert_eq!(parse_episode_number("Show.S02E05.mkv", 1), Some((2, 5)));
        assert_eq!(parse_episode_number("Show 2x07.mkv", 1), Some((2, 7)));
        assert_eq!(parse_episode_number("Show Episode 12.mkv", 3), Some((3, 12)));
        assert_eq!(parse_episode_number("[Group] Show - 08 [1080p].mkv", 1), Some((1, 8)));
        assert_eq!(parse_episode_number("Show.1920x1080.mkv", 1), None);
        assert_eq!(parse_episode_number("Extras.mkv", 1), None);
    }
}
