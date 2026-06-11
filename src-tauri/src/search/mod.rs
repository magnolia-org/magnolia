use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub title: String,
    pub size: String,
    pub seeds: u32,
    pub peers: u32,
    pub magnet_link: String,
    pub provider: String,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub quality: Option<String>,
    pub encode: Option<String>,
    pub is_batch: bool,
    pub audio_codec: Option<String>,
}

/// Metadata extracted from a release title. Used to auto-fill fields that
/// tracker extensions don't provide themselves.
#[derive(Debug, Clone, Default)]
pub struct TitleMeta {
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub quality: Option<String>,
    pub encode: Option<String>,
    pub is_batch: bool,
}

struct TitleRegexes {
    season: Regex,
    episode: Regex,
    quality: Regex,
    encode: Regex,
    batch: Regex,
}

fn title_regexes() -> &'static TitleRegexes {
    static REGEXES: OnceLock<TitleRegexes> = OnceLock::new();
    REGEXES.get_or_init(|| TitleRegexes {
        season: Regex::new(r"(?i)S(\d{1,2})|Season\s*(\d{1,2})").unwrap(),
        episode: Regex::new(r"(?i)S\d{1,2}E(\d+)|E(\d+)|Episode\s*(\d+)|\s-\s*(\d+)\s*(?:v\d)?")
            .unwrap(),
        quality: Regex::new(r"(?i)(\d{3,4}p|4K|8K|2160p|1440p|1080p|720p|480p)").unwrap(),
        encode: Regex::new(r"(?i)(x264|x265|H\.?264|H\.?265|HEVC|AVC|VP9|AV1)").unwrap(),
        batch: Regex::new(r"(?i)(batch|complete|\d+-\d+|S\d+E\d+-E?\d+)").unwrap(),
    })
}

pub fn parse_title_metadata(title: &str) -> TitleMeta {
    let r = title_regexes();

    let season = r
        .season
        .captures(title)
        .and_then(|c| c.get(1).or_else(|| c.get(2)))
        .and_then(|m| m.as_str().parse().ok());

    let episode = r
        .episode
        .captures(title)
        .and_then(|c| {
            c.get(1)
                .or_else(|| c.get(2))
                .or_else(|| c.get(3))
                .or_else(|| c.get(4))
        })
        .and_then(|m| m.as_str().parse().ok());

    let quality = r
        .quality
        .captures(title)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_uppercase());

    let encode = r
        .encode
        .captures(title)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_uppercase());

    let mut is_batch = r.batch.is_match(title);
    if season.is_some() && (episode.is_none() || title.to_lowercase().contains("season")) {
        is_batch = true;
    }

    TitleMeta {
        season,
        episode,
        quality,
        encode,
        is_batch,
    }
}

pub fn parse_audio_codec(title: &str) -> Option<String> {
    let title_upper = title.to_uppercase();

    // Check for various audio codec patterns
    if title_upper.contains("FLAC") {
        Some("FLAC".to_string())
    } else if title_upper.contains("DTS-HD") || title_upper.contains("DTS-MA") {
        Some("DTS-HD".to_string())
    } else if title_upper.contains("DTS") {
        Some("DTS".to_string())
    } else if title_upper.contains("TRUEHD") || title_upper.contains("TRUE-HD") {
        Some("TrueHD".to_string())
    } else if title_upper.contains("DD+") || title_upper.contains("DDP") || title_upper.contains("E-AC-3") || title_upper.contains("EAC3") {
        Some("E-AC3".to_string())
    } else if title_upper.contains("AC3") || title_upper.contains("AC-3") || title_upper.contains("DD5.1") || title_upper.contains("DD 5.1") || title_upper.contains("DOLBY DIGITAL") {
        Some("AC3".to_string())
    } else if title_upper.contains("AAC") {
        Some("AAC".to_string())
    } else if title_upper.contains("OPUS") {
        Some("Opus".to_string())
    } else if title_upper.contains("VORBIS") {
        Some("Vorbis".to_string())
    } else if title_upper.contains("MP3") {
        Some("MP3".to_string())
    } else {
        None
    }
}

// Check if audio codec is supported by web browsers
// Based on: https://developer.mozilla.org/en-US/docs/Web/Media/Guides/Formats/Audio_codecs
#[allow(dead_code)]
pub fn is_web_compatible(codec: Option<&str>) -> bool {
    match codec {
        // Supported web audio codecs
        Some("AAC") | Some("MP3") | Some("Opus") | Some("Vorbis") | Some("FLAC") => true,
        // Unsupported: AC3, E-AC3, DTS, DTS-HD, TrueHD
        Some("AC3") | Some("E-AC3") | Some("DTS") | Some("DTS-HD") | Some("TrueHD") => false,
        // Unknown codecs assumed incompatible
        None => true, // If no codec detected, don't filter out
        _ => false,
    }
}
