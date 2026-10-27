//! release title parsing: what a torrent holds and how it was made.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// how much of a show a release holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// nothing season-like in the name, e.g. a movie
    #[default]
    Unknown,
    Episode,
    /// a run of episodes inside one season
    Episodes,
    Season,
    Seasons,
    Complete,
}

/// what the subtitles of a release look like, when the name says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Subtitles {
    English,
    Multi,
    /// subtitles only in some language other than english
    Foreign,
    /// burned into the video
    Hardcoded,
    /// a raw, no subtitles at all
    None,
}

impl Subtitles {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "english" | "eng" | "en" => Some(Self::English),
            "multi" | "multiple" => Some(Self::Multi),
            "foreign" | "other" => Some(Self::Foreign),
            "hardcoded" | "hardsub" | "hard" => Some(Self::Hardcoded),
            "none" | "raw" => Some(Self::None),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReleaseInfo {
    pub season: Option<u32>,
    pub season_end: Option<u32>,
    pub episode: Option<u32>,
    pub episode_end: Option<u32>,
    pub scope: Scope,
    pub is_batch: bool,
    /// "8K", "4K", "1440p", "1080p", "720p" or "SD"; 2160p, uhd and 4k all land on "4K"
    pub resolution: Option<String>,
    pub upscaled: bool,
    pub source: Option<String>,
    /// cam, telesync and screener rips
    pub low_quality: bool,
    pub service: Option<String>,
    pub video_codec: Option<String>,
    pub bit_depth: Option<u32>,
    /// "DV", "HDR10+", "HDR10", "HDR" or "HLG", best first
    pub hdr: Vec<String>,
    pub audio_codec: Option<String>,
    pub audio_channels: Option<String>,
    pub atmos: bool,
    pub languages: Vec<String>,
    pub dual_audio: bool,
    pub multi_audio: bool,
    pub dubbed: bool,
    pub subtitles: Option<Subtitles>,
    pub group: Option<String>,
    pub year: Option<u32>,
    pub editions: Vec<String>,
    pub repack: bool,
}

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).unwrap()
}

struct Patterns {
    seasons_range: [Regex; 3],
    episode_se: Regex,
    episode_x: Regex,
    season: [Regex; 3],
    season_word: Regex,
    episode_range: Regex,
    episode_word: Regex,
    episode_dash: Regex,
    complete: Regex,
    batch: Regex,
    resolution_p: Regex,
    resolution_wh: Regex,
    resolution_word: Regex,
    upscaled: Regex,
    sources: Vec<(Regex, &'static str)>,
    web_bare: Regex,
    low_quality: Vec<(Regex, &'static str)>,
    transport_stream: Regex,
    service_code: Regex,
    service_name: Regex,
    video_codecs: Vec<(Regex, &'static str)>,
    bit_depth: Regex,
    hdr: Vec<(Regex, &'static str)>,
    audio_codecs: Vec<(Regex, &'static str)>,
    atmos: Regex,
    channels_after_codec: Regex,
    channels_dotted: Regex,
    channels_ch: Regex,
    subs_hardcoded: Regex,
    subs_hc: Regex,
    subs_multi: Regex,
    subs_english: Regex,
    subs_foreign: Regex,
    subs_raw: Regex,
    languages: Vec<(Regex, &'static str)>,
    dual_audio: Regex,
    multi_audio: Regex,
    dubbed: Regex,
    leading_group: Regex,
    trailing_junk: Regex,
    trailing_group: Regex,
    year: Regex,
    editions: Vec<(Regex, &'static str)>,
    repack: Regex,
    name_marker: Regex,
}

fn patterns() -> &'static Patterns {
    static PATTERNS: OnceLock<Patterns> = OnceLock::new();
    PATTERNS.get_or_init(|| Patterns {
        seasons_range: [
            re(r"(?i)\bS(\d{1,2})(?:[\s.]?E\d{1,4})?[\s.]*(?:-|~|&|\+|to)[\s.]*S(\d{1,2})\b"),
            re(r"(?i)\bSeasons?[\s.]*(\d{1,2})[\s.]*(?:-|~|&|\+|to|and)[\s.]*(?:Season[\s.]*)?(\d{1,2})\b"),
            re(r"(?i)\bS(\d{1,2})-(\d{1,2})\b"),
        ],
        episode_se: re(r"(?i)\bS(\d{1,2})[\s.]?E(\d{1,4})(?:v\d)?(?:[\s.]?(?:-|~|E|-E)[\s.]?E?(\d{1,4}))?\b"),
        episode_x: re(r"\b(\d{1,2})x(\d{2,3})\b"),
        season: [
            re(r"(?i)\bS(\d{1,2})(?:v\d+)?\b"),
            re(r"(?i)\bSeason[\s.]*(\d{1,2})\b"),
            re(r"(?i)\b(\d{1,2})(?:st|nd|rd|th)[\s.]+Season\b"),
        ],
        season_word: re(r"(?i)\bSeason[\s.]+(one|two|three|four|five|six|seven|eight|nine|ten)\b"),
        episode_range: re(r"(?:^|[\s\[(])(\d{1,4})\s?(?:-|~)\s?(\d{1,4})(?:$|[\s\])])"),
        episode_word: re(r"(?i)\bEp(?:isode)?[\s.]*(\d{1,4})(?:v\d)?\b"),
        episode_dash: re(r"\s-\s(\d{1,4})(?:v\d+)?(?:$|[\s\[(.])"),
        complete: re(r"(?i)\b(?:complete|integrale|all[\s.]+seasons|full[\s.]+series|collection|anthology)\b"),
        batch: re(r"(?i)\b(?:batch|bd[\s.-]?box)\b"),
        resolution_p: re(r"(?i)\b(\d{3,4})[pi]\b"),
        resolution_wh: re(r"\b(\d{3,4})x(\d{3,4})\b"),
        resolution_word: re(r"(?i)\b(8K|4K|UHD|FHD|QHD)\b"),
        upscaled: re(r"(?i)\b(?:AI[\s.-]?)?upscaled?\b|\bAI[\s.-]?enhanced\b"),
        sources: vec![
            (re(r"(?i)\b(?:BD|BluRay|UHD)?[\s.-]?Remux\b"), "Remux"),
            (re(r"(?i)\bWEB[\s.-]?(?:DL)?[\s.-]?Rip\b"), "WEBRip"),
            (re(r"(?i)\bWEB[\s.-]?DL\b"), "WEB-DL"),
            (re(r"(?i)\b(?:Blu[\s.-]?Ray|BDRip|BRRip|BDMV|BD(?:[\s.-]?\d{3,4}p)?)\b"), "BluRay"),
            (re(r"(?i)\b(?:HDTV|PDTV|DSR|TVRip|SATRip|DTHRip|MPEG-?2[\s.-]?TS)\b"), "HDTV"),
            (re(r"(?i)\bDVD(?:Rip|R|5|9)?\b"), "DVD"),
            (re(r"(?i)\bHDRip\b"), "HDRip"),
        ],
        web_bare: re(r"\bWEB\b"),
        transport_stream: re(r"(?i)\bMPEG-?2[\s.-]?TS\b|\.ts\s*$"),
        low_quality: vec![
            (re(r"(?i)\b(?:CAM(?:Rip)?|HD[\s.-]?CAM)\b"), "CAM"),
            (re(r"(?i)\b(?:TELESYNC|HD[\s.-]?TS|TS[\s.-]?Rip)\b|\bTS\b"), "Telesync"),
            (re(r"(?i)\b(?:TELECINE|HD[\s.-]?TC)\b|\bTC\b"), "Telecine"),
            (re(r"(?i)\b(?:SCREENER|DVD[\s.-]?SCR|PDVD|PreDVD(?:Rip)?|WORKPRINT)\b|\bSCR\b|\bR5\b"), "Screener"),
        ],
        service_code: re(
            r"\b(AMZN|NF|NFLX|DSNP|DSNY|HMAX|MAX|ATVP|HULU|PCOK|PMTP|PMNT|CR|FUNI|HIDIVE|HIDI|ADN|BILI|B-Global|YT|iT|STAN|CRAV|ROKU|SHO|STZ|SKST|iP|MUBI|ABEMA)[\s.]+(?:WEB|\(?\d{3,4}p)",
        ),
        service_name: re(r"(?i)\b(Netflix|Crunchyroll|HIDIVE|Funimation|Disney\+|Bilibili|Amazon[\s.]Prime)"),
        video_codecs: vec![
            (re(r"(?i)\b(?:[xh][\s.]?265|HEVC)\b"), "HEVC"),
            (re(r"(?i)\b(?:[xh][\s.]?264|AVC)\b"), "AVC"),
            (re(r"(?i)\bAV1\b"), "AV1"),
            (re(r"(?i)\bVP9\b"), "VP9"),
            (re(r"(?i)\b(?:XviD|DivX)\b"), "XviD"),
            (re(r"(?i)\bMPEG[\s.-]?2\b"), "MPEG-2"),
        ],
        bit_depth: re(r"(?i)\b(8|10|12)[\s.-]?bits?\b|\b(Hi10P?|Main[\s.]?10)\b"),
        hdr: vec![
            (re(r"(?i)\b(?:DV|DoVi|Dolby[\s.]?Vision)\b"), "DV"),
            (re(r"(?i)\bHDR10(?:\+|Plus\b|P\b)"), "HDR10+"),
            (re(r"(?i)\bHDR10\b"), "HDR10"),
            (re(r"(?i)\bHDR\b"), "HDR"),
            (re(r"(?i)\bHLG\b"), "HLG"),
        ],
        audio_codecs: vec![
            (re(r"(?i)\bTrue[\s.-]?HD\b"), "TrueHD"),
            (re(r"(?i)\bDTS[\s.-]?X\b"), "DTS:X"),
            (re(r"(?i)\bDTS[\s.-]?HD(?:[\s.-]?MA)?\b|\bDTS[\s.-]?MA\b"), "DTS-HD MA"),
            (re(r"(?i)\bDTS(?:[\s.-]?ES)?(?:\d|\b)"), "DTS"),
            (re(r"(?i)\b(?:DDP|EAC3|E-AC-?3)|\bDD\+"), "DD+"),
            (re(r"(?i)\b(?:DD|AC-?3|Dolby[\s.]?Digital)(?:\d|\b)"), "DD"),
            (re(r"(?i)\bFLAC(?:\d|\b)"), "FLAC"),
            (re(r"(?i)\bOpus(?:\d|\b)"), "Opus"),
            (re(r"(?i)\bL?PCM\b"), "PCM"),
            (re(r"(?i)\bAAC(?:\d|\b)"), "AAC"),
            (re(r"(?i)\bMP3\b"), "MP3"),
            (re(r"(?i)\bVorbis\b"), "Vorbis"),
        ],
        atmos: re(r"(?i)\bAtmos\b"),
        channels_after_codec: re(
            r"(?i)(?:DDP?|DD\+|AAC|DTS(?:[\s.-]?(?:HD|MA|X|ES))*|FLAC|TrueHD|Atmos|Opus|AC-?3|EAC3|L?PCM)[\s.-]*([12578])[\s.]([01])\b",
        ),
        channels_dotted: re(r"(?:^|[^\d.])([57])\.1(?:$|[^\d.])"),
        channels_ch: re(r"(?i)\b([2678])\s?CH\b"),
        subs_hardcoded: re(r"(?i)\bHard[\s.-]?(?:sub(?:bed|s)?|coded(?:[\s.-]?sub(?:s|titles?)?)?)\b"),
        subs_hc: re(r"\bHC\b"),
        subs_multi: re(r"(?i)\bMulti(?:ple)?[\s.-]?Sub(?:s|titles?|bed)?\b"),
        subs_english: re(
            r"(?i)\bE[\s.-]?Subs?\b|\bEng(?:lish)?[\s.-]?(?:Soft[\s.-]?)?Sub(?:s|bed|titles?)?\b|\bSub(?:s|titles?)?[\s.-]?Eng(?:lish)?\b|\bSubbed\b|\bSoft[\s.-]?Subs?\b",
        ),
        subs_foreign: re(
            r"(?i)\b(?:VOSTFR|VOST|SUBFRENCH|Legendado|Subtitulado|Sub[\s.-]?ITA|Sub[\s.-]?Esp|Sub[\s.-]?Spa)\b|[简繁]|中字|雙語|双语|字幕|\b(?:CHS|CHT|BIG5)\b",
        ),
        subs_raw: re(r"(?i)\bRAWS?\b"),
        languages: vec![
            (re(r"(?i)\b(?:ENG|English)\b"), "English"),
            (re(r"(?i)\b(?:JAP|JPN|Japanese)\b"), "Japanese"),
            (re(r"(?i)\b(?:Hin|Hindi)\b"), "Hindi"),
            (re(r"(?i)\b(?:FRE|FRA|French|TRUEFRENCH|VFF|VFQ|VF2|VFI)\b"), "French"),
            (re(r"(?i)\b(?:GER|German|Deutsch)\b"), "German"),
            (re(r"(?i)\b(?:ITA|Italian)\b"), "Italian"),
            (re(r"(?i)\b(?:SPA|ESP|Spanish|Castellano|Latino)\b"), "Spanish"),
            (re(r"(?i)\b(?:POR|Portuguese|PT-?BR|Dublado)\b"), "Portuguese"),
            (re(r"(?i)\b(?:RUS|Russian)\b"), "Russian"),
            (re(r"(?i)\b(?:KOR|Korean)\b"), "Korean"),
            (re(r"(?i)\b(?:CHI|Chinese|Mandarin|Cantonese)\b"), "Chinese"),
            (re(r"(?i)\b(?:TAM|Tamil)\b"), "Tamil"),
            (re(r"(?i)\b(?:TEL|Telugu)\b"), "Telugu"),
            (re(r"(?i)\b(?:POL|Polish|PLDUB)\b"), "Polish"),
            (re(r"(?i)\b(?:TUR|Turkish)\b"), "Turkish"),
            (re(r"(?i)\b(?:ARA|Arabic)\b"), "Arabic"),
            (re(r"(?i)\b(?:UKR|Ukrainian)\b"), "Ukrainian"),
        ],
        dual_audio: re(r"(?i)\bDual[\s.-]?Audio\b|\bDUAL\b"),
        multi_audio: re(r"(?i)\bMulti[\s.-]?Audio\b|\bMULTI\b"),
        dubbed: re(r"(?i)\b(?:Dub(?:bed)?|Dubs)\b"),
        leading_group: re(r"^\s*[\[【]([^\]】]{1,40})[\]】]"),
        trailing_junk: re(
            r"(?i)(?:\s*[\[(][^\])]*[\])]|\.(?:mkv|mp4|avi|m4v|ts)|\s+(?:EZTV|TGx|RARBG|ettv|eztv\.re))+\s*$",
        ),
        trailing_group: re(r"(?:-|\s-\s)([A-Za-z][A-Za-z0-9]{1,19})$"),
        year: re(r"\b(19[2-9]\d|20[0-4]\d)\b"),
        editions: vec![
            (re(r"(?i)\bExtended(?:[\s.](?:Cut|Edition))?\b"), "Extended"),
            (re(r"(?i)\bDirector'?s[\s.]Cut\b"), "Director's Cut"),
            (re(r"(?i)\bUncut\b"), "Uncut"),
            (re(r"(?i)\bUnrated\b"), "Unrated"),
            (re(r"(?i)\bTheatrical(?:[\s.]Cut)?\b"), "Theatrical"),
            (re(r"(?i)\bIMAX\b"), "IMAX"),
            (re(r"(?i)\bRemaster(?:ed)?\b"), "Remastered"),
            (re(r"(?i)\bCriterion\b"), "Criterion"),
            (re(r"(?i)\bSpecial[\s.]Edition\b"), "Special Edition"),
            (re(r"(?i)\bFinal[\s.]Cut\b"), "Final Cut"),
            (re(r"(?i)\bOpen[\s.]Matte\b"), "Open Matte"),
        ],
        repack: re(r"(?i)\b(?:REPACK\d?|PROPER|RERIP)\b|\dv[2-9]\b"),
        name_marker: re(
            r"(?i)\bS\d{1,2}(?:[\s.]?E\d{1,4})?\b|\b\d{1,2}x\d{2,3}\b|\bSeasons?[\s.]*\d|\b\d{1,2}(?:st|nd|rd|th)[\s.]+Season|\b(?:19[2-9]\d|20[0-4]\d)\b|\b\d{3,4}[pi]\b|\b(?:4K|UHD)\b|\bComplete\b|\s-\s\d{1,4}\b|[\[(【]|\b(?:BluRay|WEB[\s.-]?DL|WEBRip|HDTV|BDRip|DVDRip|x26[45]|HEVC|REPACK|PROPER)\b",
        ),
    })
}

/// fansub groups whose releases carry english (or many) soft subtitles.
const ENGLISH_SUB_GROUPS: &[&str] = &[
    "subsplease", "horriblesubs", "judas", "ember", "asw", "yameii", "anime time", "dkb", "commie",
    "gjm", "kametsu", "cleo", "golumpa", "sallysubs", "chihiro", "coalgirls", "doki", "mtbb",
];
const MULTI_SUB_GROUPS: &[&str] = &["erai-raws", "toonshub", "varyg", "tsundere-raws"];

fn first_u32(re: &Regex, text: &str) -> Option<u32> {
    re.captures(text)?.get(1)?.as_str().parse().ok()
}

fn word_number(word: &str) -> Option<u32> {
    let n = match word.to_ascii_lowercase().as_str() {
        "one" => 1,
        "two" => 2,
        "three" => 3,
        "four" => 4,
        "five" => 5,
        "six" => 6,
        "seven" => 7,
        "eight" => 8,
        "nine" => 9,
        "ten" => 10,
        _ => return None,
    };
    Some(n)
}

/// buckets a pixel height into the label shown to the user, so odd heights
/// (2140p, 1088p, 3840x1600 crops) group with the standard they belong to.
pub fn resolution_label(height: u32) -> Option<&'static str> {
    Some(match height {
        h if h >= 3200 => "8K",
        h if h >= 1800 => "4K",
        h if h >= 1300 => "1440p",
        h if h >= 900 => "1080p",
        h if h >= 650 => "720p",
        h if h >= 240 => "SD",
        _ => return None,
    })
}

/// the canonical label for whatever resolution text a tracker hands over.
pub fn normalize_resolution(text: &str) -> Option<String> {
    parse_resolution(text).or_else(|| match text.trim().to_ascii_uppercase().as_str() {
        "SD" | "480" | "576" => Some("SD".to_string()),
        _ => None,
    })
}

fn parse_resolution(title: &str) -> Option<String> {
    let p = patterns();
    // crops like 3840x1600 still count by width
    if let Some(c) = p.resolution_wh.captures(title) {
        let w: u32 = c[1].parse().ok()?;
        let h: u32 = c[2].parse().ok()?;
        let by_width = match w {
            w if w >= 7000 => 4320,
            w if w >= 3500 => 2160,
            w if w >= 2400 => 1440,
            w if w >= 1800 => 1080,
            w if w >= 1200 => 720,
            _ => h,
        };
        return resolution_label(by_width.max(h)).map(String::from);
    }
    if let Some(h) = p
        .resolution_p
        .captures_iter(title)
        .filter_map(|c| c[1].parse::<u32>().ok())
        .find(|h| (240..=4400).contains(h))
    {
        return resolution_label(h).map(String::from);
    }
    p.resolution_word.captures(title).map(|c| {
        match c[1].to_ascii_uppercase().as_str() {
            "8K" => "8K",
            "4K" | "UHD" => "4K",
            "QHD" => "1440p",
            _ => "1080p",
        }
        .to_string()
    })
}

fn service_label(code: &str) -> &'static str {
    match code.to_ascii_lowercase().as_str() {
        "amzn" | "amazon prime" | "amazon.prime" => "Amazon",
        "nf" | "nflx" | "netflix" => "Netflix",
        "dsnp" | "dsny" | "disney+" => "Disney+",
        "hmax" | "max" => "Max",
        "atvp" => "Apple TV+",
        "hulu" => "Hulu",
        "pcok" => "Peacock",
        "pmtp" | "pmnt" => "Paramount+",
        "cr" | "crunchyroll" => "Crunchyroll",
        "funi" | "funimation" => "Funimation",
        "hidive" | "hidi" => "HIDIVE",
        "adn" => "ADN",
        "bili" | "b-global" | "bilibili" => "Bilibili",
        "yt" => "YouTube",
        "it" => "iTunes",
        "stan" => "Stan",
        "crav" => "Crave",
        "roku" => "Roku",
        "sho" => "Showtime",
        "stz" => "Starz",
        "skst" => "SkyShowtime",
        "ip" => "BBC iPlayer",
        "mubi" => "MUBI",
        "abema" => "Abema",
        _ => "WEB",
    }
}

/// a group name that is really a codec or source tag left at the end of a title.
fn is_tag_word(word: &str) -> bool {
    const TAGS: &[&str] = &[
        "dl", "rip", "web", "webrip", "hdtv", "bluray", "x264", "x265", "h264", "h265", "hevc", "avc", "aac",
        "dts", "ac3", "eac3", "atmos", "remux", "hdr", "sdr", "dv", "esub", "esubs", "subs", "sub", "audio",
        "dual", "multi", "proper", "repack", "complete", "batch", "uncut", "extended", "eng", "english",
    ];
    TAGS.contains(&word.to_ascii_lowercase().as_str()) || parse_resolution(word).is_some()
}

fn find_group(title: &str) -> Option<String> {
    let p = patterns();
    if let Some(c) = p.leading_group.captures(title) {
        let g = c[1].trim();
        let lower = g.to_ascii_lowercase();
        let looks_like_site = lower.contains(".com") || lower.contains(".org") || lower.contains("www.");
        if !g.is_empty() && !looks_like_site && parse_resolution(g).is_none() {
            return Some(g.to_string());
        }
    }
    let trimmed = p.trailing_junk.replace(title.trim(), "");
    let c = p.trailing_group.captures(trimmed.trim_end())?;
    let g = &c[1];
    (!is_tag_word(g)).then(|| g.to_string())
}

/// where the show or movie name ends and the release tags begin.
fn name_end(title: &str) -> (usize, usize) {
    let p = patterns();
    let start = p.leading_group.find(title).map_or(0, |m| m.end());
    let rest = &title[start..];
    let end = p
        .name_marker
        .find_iter(rest)
        .map(|m| m.start())
        .find(|pos| !rest[..*pos].trim().is_empty())
        .unwrap_or(rest.len());
    (start, start + end)
}

/// lowercase words of a name, with punctuation dropped, for comparing titles.
pub fn normalize_name(name: &str) -> String {
    let cleaned: String = name
        .replace('&', " and ")
        .chars()
        .filter(|c| !matches!(c, '\'' | '’' | '`'))
        .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { ' ' })
        .collect();
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    let words = match words.first() {
        Some(&"the") if words.len() > 1 => &words[1..],
        _ => &words[..],
    };
    words.join(" ")
}

/// the normalized show or movie name at the front of a release title.
pub fn release_name(title: &str) -> String {
    let (start, end) = name_end(title);
    normalize_name(&title[start..end])
}

/// the parts of a title after the name, where language and service tags can
/// appear without being mistaken for words of the name ("the french dispatch").
fn tags_part(title: &str) -> &str {
    let (_, end) = name_end(title);
    &title[end..]
}

fn parse_episodes(title: &str, info: &mut ReleaseInfo) {
    let p = patterns();

    for range in &p.seasons_range {
        if let Some(c) = range.captures(title) {
            let (a, b): (u32, u32) = (c[1].parse().unwrap_or(0), c[2].parse().unwrap_or(0));
            if b > a {
                info.season = Some(a);
                info.season_end = Some(b);
                return;
            }
        }
    }

    if let Some(c) = p.episode_se.captures(title) {
        info.season = c[1].parse().ok();
        info.episode = c[2].parse().ok();
        info.episode_end = c
            .get(3)
            .and_then(|m| m.as_str().parse().ok())
            .filter(|end| Some(*end) > info.episode);
        return;
    }
    if let Some(c) = p.episode_x.captures(title) {
        info.season = c[1].parse().ok();
        info.episode = c[2].parse().ok();
        return;
    }

    info.season = p
        .season
        .iter()
        .find_map(|re| first_u32(re, title))
        .or_else(|| word_number(p.season_word.captures(title)?.get(1)?.as_str()));

    // anime style numbering: "[29-38]", "- 05", "Episode 5"
    let range = p.episode_range.captures_iter(title).find_map(|c| {
        let a: u32 = c[1].parse().ok()?;
        let b: u32 = c[2].parse().ok()?;
        let is_years = a >= 1900 || b >= 1900;
        (!is_years && b > a).then_some((a, b))
    });
    if let Some((a, b)) = range {
        info.episode = Some(a);
        info.episode_end = Some(b);
        return;
    }
    info.episode = first_u32(&p.episode_word, title).or_else(|| {
        let ep = first_u32(&p.episode_dash, title)?;
        (ep < 1900).then_some(ep)
    });
}

fn parse_audio(title: &str, info: &mut ReleaseInfo) {
    let p = patterns();
    info.audio_codec = p
        .audio_codecs
        .iter()
        .find(|(re, _)| re.is_match(title))
        .map(|(_, name)| name.to_string());
    info.atmos = p.atmos.is_match(title);
    info.audio_channels = p
        .channels_after_codec
        .captures(title)
        .map(|c| format!("{}.{}", &c[1], &c[2]))
        .or_else(|| p.channels_dotted.captures(title).map(|c| format!("{}.1", &c[1])))
        .or_else(|| {
            let ch = p.channels_ch.captures(title)?;
            Some(match &ch[1] {
                "2" => "2.0",
                "6" => "5.1",
                _ => "7.1",
            }
            .to_string())
        });
}

fn parse_subtitles(title: &str, group: Option<&str>) -> Option<Subtitles> {
    let p = patterns();
    if p.subs_hardcoded.is_match(title) || p.subs_hc.is_match(title) {
        return Some(Subtitles::Hardcoded);
    }
    if p.subs_multi.is_match(title) {
        return Some(Subtitles::Multi);
    }
    if p.subs_english.is_match(title) {
        return Some(Subtitles::English);
    }
    if p.subs_foreign.is_match(title) {
        return Some(Subtitles::Foreign);
    }
    let group = group.map(|g| g.to_ascii_lowercase());
    if let Some(g) = group.as_deref() {
        if MULTI_SUB_GROUPS.contains(&g) {
            return Some(Subtitles::Multi);
        }
        if ENGLISH_SUB_GROUPS.contains(&g) {
            return Some(Subtitles::English);
        }
        if g.ends_with("raws") || g.ends_with("raw") {
            return Some(Subtitles::None);
        }
    }
    p.subs_raw.is_match(title).then_some(Subtitles::None)
}

fn parse_languages(tags: &str, info: &mut ReleaseInfo) {
    let p = patterns();
    // subtitle tags name languages too ("eng subs"), which say nothing about the audio
    let mut audio_tags = tags.to_string();
    for re in [&p.subs_english, &p.subs_multi, &p.subs_foreign] {
        audio_tags = re.replace_all(&audio_tags, " ").into_owned();
    }
    info.languages = p
        .languages
        .iter()
        .filter(|(re, _)| re.is_match(&audio_tags))
        .map(|(_, name)| name.to_string())
        .collect();
    info.dual_audio = p.dual_audio.is_match(&audio_tags);
    info.multi_audio = p.multi_audio.is_match(&audio_tags);
    info.dubbed = p.dubbed.is_match(&audio_tags);
}

/// a long language list beside multi subs ("[ENG][POR-BR][SPA-LA]") names the
/// subtitles, not the audio.
fn drop_subtitle_languages(info: &mut ReleaseInfo) {
    if info.subtitles == Some(Subtitles::Multi) && info.languages.len() > 2 && !info.dual_audio && !info.multi_audio {
        info.languages.clear();
    }
}

fn scope_of(info: &ReleaseInfo, complete: bool, batch: bool) -> Scope {
    if info.season_end.is_some() {
        Scope::Seasons
    } else if info.episode.is_some() {
        if info.episode_end.is_some() {
            Scope::Episodes
        } else {
            Scope::Episode
        }
    } else if info.season.is_some() {
        Scope::Season
    } else if complete {
        Scope::Complete
    } else if batch {
        // a nameless batch is nearly always one whole season
        Scope::Season
    } else {
        Scope::Unknown
    }
}

pub fn parse_release(title: &str) -> ReleaseInfo {
    let p = patterns();
    let title = title.replace('_', " ");
    let title = title.as_str();
    let tags = tags_part(title);
    let mut info = ReleaseInfo::default();

    parse_episodes(title, &mut info);
    let complete = p.complete.is_match(title);
    let batch = p.batch.is_match(title);
    info.scope = scope_of(&info, complete, batch);
    info.is_batch = !matches!(info.scope, Scope::Episode | Scope::Unknown);

    info.resolution = parse_resolution(title);
    info.upscaled = p.upscaled.is_match(title);

    // "mpeg2 ts" and ".ts" are tv captures in a transport stream, not telesyncs
    let capture_tags = p.transport_stream.replace_all(tags, " ");
    if let Some((_, name)) = p.low_quality.iter().find(|(re, _)| re.is_match(&capture_tags)) {
        info.source = Some(name.to_string());
        info.low_quality = true;
    } else {
        info.source = p
            .sources
            .iter()
            .find(|(re, _)| re.is_match(title))
            .map(|(_, name)| name.to_string())
            .or_else(|| p.web_bare.is_match(tags).then(|| "WEB-DL".to_string()));
    }
    info.service = p
        .service_code
        .captures(tags)
        .map(|c| service_label(&c[1]))
        .or_else(|| p.service_name.captures(tags).map(|c| service_label(&c[1])))
        .filter(|s| *s != "WEB")
        .map(String::from);

    info.video_codec = p
        .video_codecs
        .iter()
        .find(|(re, _)| re.is_match(title))
        .map(|(_, name)| name.to_string());
    info.bit_depth = p.bit_depth.captures(title).and_then(|c| match c.get(1) {
        Some(bits) => bits.as_str().parse().ok(),
        None => Some(10),
    });

    let mut hdr: Vec<String> = p
        .hdr
        .iter()
        .filter(|(re, _)| re.is_match(tags))
        .map(|(_, name)| name.to_string())
        .collect();
    if hdr.iter().any(|h| h == "HDR10+") {
        hdr.retain(|h| h != "HDR10" && h != "HDR");
    } else if hdr.iter().any(|h| h == "HDR10") {
        hdr.retain(|h| h != "HDR");
    }
    info.hdr = hdr;

    parse_audio(title, &mut info);
    info.group = find_group(title);
    info.subtitles = parse_subtitles(title, info.group.as_deref());
    parse_languages(tags, &mut info);
    drop_subtitle_languages(&mut info);

    info.year = p
        .year
        .captures_iter(title)
        .filter(|c| c.get(0).map_or(false, |m| m.start() > 0))
        .filter_map(|c| c[1].parse().ok())
        .last();
    info.editions = p
        .editions
        .iter()
        .filter(|(re, _)| re.is_match(tags))
        .map(|(_, name)| name.to_string())
        .collect();
    info.repack = p.repack.is_match(title);

    info
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_episodes() {
        let r = parse_release("Breaking Bad S01E05 Gray Matter 2160p NF WEB-DL DDP5 1 H 265-XEBEC");
        assert_eq!((r.season, r.episode, r.episode_end), (Some(1), Some(5), None));
        assert_eq!(r.scope, Scope::Episode);
        assert!(!r.is_batch);
        assert_eq!(r.resolution.as_deref(), Some("4K"));
        assert_eq!(r.source.as_deref(), Some("WEB-DL"));
        assert_eq!(r.service.as_deref(), Some("Netflix"));
        assert_eq!(r.video_codec.as_deref(), Some("HEVC"));
        assert_eq!(r.audio_codec.as_deref(), Some("DD+"));
        assert_eq!(r.audio_channels.as_deref(), Some("5.1"));
        assert_eq!(r.group.as_deref(), Some("XEBEC"));
    }

    #[test]
    fn odd_resolutions_group_together() {
        for title in ["Movie 2019 2160p", "Movie 2019 2140p", "Movie 2019 4K", "Movie 2019 UHD BluRay", "Movie 3840x1600"] {
            assert_eq!(parse_release(title).resolution.as_deref(), Some("4K"), "{}", title);
        }
        assert_eq!(parse_release("Movie 1920x1080").resolution.as_deref(), Some("1080p"));
        assert_eq!(parse_release("Movie 1088p").resolution.as_deref(), Some("1080p"));
        assert_eq!(parse_release("Movie 576p DVD").resolution.as_deref(), Some("SD"));
        assert_eq!(parse_release("One Piece - 1080 [720p]").resolution.as_deref(), Some("720p"));
    }

    #[test]
    fn anime_releases() {
        let r = parse_release("[SubsPlease] Sousou no Frieren - 05 (1080p) [ABCD1234].mkv");
        assert_eq!(r.episode, Some(5));
        assert_eq!(r.season, None);
        assert_eq!(r.scope, Scope::Episode);
        assert_eq!(r.group.as_deref(), Some("SubsPlease"));
        assert_eq!(r.subtitles, Some(Subtitles::English));

        let r = parse_release("[SubsPlease] Sousou no Frieren S2 (01-10) (1080p) [Batch]");
        assert_eq!((r.season, r.episode, r.episode_end), (Some(2), Some(1), Some(10)));
        assert_eq!(r.scope, Scope::Episodes);

        let r = parse_release("[Erai-raws] Sousou no Frieren - 05 [1080p][Multiple Subtitle] [ENG][POR-BR][SPA-LA][SPA][ARA][FRE][GER][ITA][RUS]");
        assert_eq!(r.subtitles, Some(Subtitles::Multi));
        assert!(r.languages.is_empty());

        let r = parse_release("[Erai-raws] Sousou no Frieren 2nd Season [1080p CR WEBRip HEVC AAC][MultiSub]");
        assert_eq!(r.season, Some(2));
        assert_eq!(r.scope, Scope::Season);
        assert_eq!(r.subtitles, Some(Subtitles::Multi));
        assert_eq!(r.source.as_deref(), Some("WEBRip"));
        assert_eq!(r.service.as_deref(), Some("Crunchyroll"));

        let r = parse_release("【喵萌奶茶屋】★01月新番★[葬送的芙莉蓮 / Sousou no Frieren][29-38][BDRip][1080p][繁日雙語]");
        assert_eq!((r.episode, r.episode_end), (Some(29), Some(38)));
        assert_eq!(r.subtitles, Some(Subtitles::Foreign));
        assert_eq!(r.source.as_deref(), Some("BluRay"));
    }

    #[test]
    fn multi_season_packs() {
        let r = parse_release("[Xspitfire911] Sousou No Frieren S01 + S02 BDRIP 1080p X265 10bit VOSTFR");
        assert_eq!((r.season, r.season_end), (Some(1), Some(2)));
        assert_eq!(r.scope, Scope::Seasons);
        assert_eq!(r.bit_depth, Some(10));
        assert_eq!(r.subtitles, Some(Subtitles::Foreign));

        let r = parse_release("Breaking Bad Complete Series S01-S05 1080p BluRay x265");
        assert_eq!((r.season, r.season_end), (Some(1), Some(5)));
        assert_eq!(parse_release("The Office Complete Series 720p").scope, Scope::Complete);
        assert_eq!(parse_release("Show Season 1 Complete 1080p").scope, Scope::Season);
    }

    #[test]
    fn hdr_audio_and_subtitles() {
        let r = parse_release("Dune.Part.Two.2024.2160p.UHD.BluRay.REMUX.DV.HDR10+.TrueHD.Atmos.7.1-FGT");
        assert_eq!(r.hdr, vec!["DV", "HDR10+"]);
        assert_eq!(r.source.as_deref(), Some("Remux"));
        assert_eq!(r.audio_codec.as_deref(), Some("TrueHD"));
        assert!(r.atmos);
        assert_eq!(r.audio_channels.as_deref(), Some("7.1"));
        assert_eq!(r.year, Some(2024));
        assert_eq!(r.group.as_deref(), Some("FGT"));

        let r = parse_release("Breaking Bad S01 720p BRRip Hindi-English ESUB - Cukister");
        assert_eq!(r.subtitles, Some(Subtitles::English));
        assert_eq!(r.languages, vec!["English", "Hindi"]);
        assert_eq!(r.group.as_deref(), Some("Cukister"));

        let r = parse_release("Breaking Bad  S05  2008  2160P Ai Upscaled BluRay 60FPS H265 SDR DTS5 1 Marjenbo");
        assert!(r.upscaled);
        assert!(r.hdr.is_empty());
        assert_eq!(r.audio_codec.as_deref(), Some("DTS"));
        assert_eq!(r.audio_channels.as_deref(), Some("5.1"));

        assert_eq!(parse_release("Movie 2020 1080p WEB-DL HC").subtitles, Some(Subtitles::Hardcoded));
        assert!(parse_release("Movie 2020 HDRip x264").hdr.is_empty());
    }

    #[test]
    fn names_in_titles_are_not_tags() {
        let r = parse_release("The French Dispatch 2021 1080p WEB-DL");
        assert!(r.languages.is_empty());
        assert_eq!(release_name("The French Dispatch 2021 1080p WEB-DL"), "french dispatch");
        assert_eq!(release_name("[SubsPlease] Sousou no Frieren - 05 (1080p)"), "sousou no frieren");
        assert_eq!(release_name("Breaking.Bad.S01E05.720p"), "breaking bad");
        assert_eq!(release_name("2012 2009 1080p BluRay"), "2012");
        assert_eq!(parse_release("2012 2009 1080p BluRay").year, Some(2009));
        assert_eq!(parse_release("Charlotte's Web 2006 DVDRip").source.as_deref(), Some("DVD"));
    }

    #[test]
    fn low_quality_rips() {
        let capture = parse_release("[shincaps] Sousou no Frieren - 05 (AT-X 1440x1080 MPEG2 TS).ts");
        assert!(!capture.low_quality);
        assert_eq!(capture.source.as_deref(), Some("HDTV"));
        assert_eq!(capture.resolution.as_deref(), Some("1080p"));
        assert!(parse_release("Movie 2025 HDCAM x264").low_quality);
        assert!(parse_release("Movie 2025 TS 720p").low_quality);
        assert!(!parse_release("Movie 2025 1080p WEB-DL").low_quality);
    }
}
