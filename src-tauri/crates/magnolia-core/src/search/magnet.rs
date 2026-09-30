//! magnet link helpers.

use regex::Regex;
use std::sync::OnceLock;

use super::{parse_audio_codec, parse_title_metadata, SearchResult};

/// lowercase info hash of a magnet, used to drop duplicate results.
pub fn extract_info_hash(magnet: &str) -> Option<String> {
    // strip the scheme so `xt=` matches even as the first parameter ("magnet:?xt=urn:btih:...")
    let params = magnet.split_once('?').map(|(_, q)| q).unwrap_or(magnet);
    params
        .split('&')
        .find(|part| part.starts_with("xt=urn:btih:"))
        .and_then(|part| part.strip_prefix("xt=urn:btih:"))
        .map(|hash| hash.to_lowercase())
}

pub fn is_valid_magnet(link: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)^magnet:\?xt=urn:[a-z0-9]+:[a-z0-9]{32,}").unwrap())
        .is_match(link)
}

/// the `dn` (display name) of a magnet, if it has one.
pub fn display_name(link: &str) -> Option<String> {
    let params = link.split_once('?').map(|(_, q)| q).unwrap_or(link);
    let raw = params.split('&').find_map(|part| part.strip_prefix("dn="))?;
    let decoded = urlencoding::decode(&raw.replace('+', " ")).ok()?.into_owned();
    (!decoded.is_empty()).then_some(decoded)
}

/// a search result for a magnet the user pasted in.
pub fn result_from_magnet(link: &str) -> Result<SearchResult, String> {
    let link = link.trim();
    if !is_valid_magnet(link) {
        return Err("Invalid magnet link format".to_string());
    }
    let title = display_name(link).unwrap_or_else(|| "Custom Magnet Link".to_string());
    let meta = parse_title_metadata(&title);
    Ok(SearchResult {
        size: "Unknown".to_string(),
        seeds: 0,
        peers: 0,
        magnet_link: link.to_string(),
        provider: "custom".to_string(),
        season: meta.season,
        episode: meta.episode,
        quality: meta.quality,
        encode: meta.encode,
        is_batch: meta.is_batch,
        audio_codec: parse_audio_codec(&title),
        title,
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_info_hash_handles_leading_scheme() {
        // hash as the first magnet parameter (the common case)
        assert_eq!(
            extract_info_hash("magnet:?xt=urn:btih:ABCDEF123456&dn=Some%20Title&tr=udp%3A%2F%2Ffoo"),
            Some("abcdef123456".to_string())
        );
        // hash not first
        assert_eq!(
            extract_info_hash("magnet:?dn=Some%20Title&xt=urn:btih:ABCDEF123456"),
            Some("abcdef123456".to_string())
        );
        assert_eq!(extract_info_hash("magnet:?dn=No%20Hash"), None);
    }

    #[test]
    fn pasted_magnets_are_validated_and_named() {
        let hash = "0123456789abcdef0123456789abcdef01234567";
        let link = format!("magnet:?xt=urn:btih:{}&dn=Show+S01E02+1080p", hash);
        let result = result_from_magnet(&link).unwrap();
        assert_eq!(result.title, "Show S01E02 1080p");
        assert_eq!(result.season, Some(1));
        assert_eq!(result.episode, Some(2));
        assert_eq!(result.provider, "custom");

        assert!(result_from_magnet("magnet:?xt=urn:btih:short").is_err());
        let unnamed = result_from_magnet(&format!("magnet:?xt=urn:btih:{}", hash)).unwrap();
        assert_eq!(unnamed.title, "Custom Magnet Link");
    }
}
