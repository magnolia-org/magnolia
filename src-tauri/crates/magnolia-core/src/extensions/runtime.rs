//! Rhai engine setup and script execution for extensions.
//!
//! Scripts are sandboxed: they have no filesystem or process access, only the
//! host functions registered here (HTTP, regex, JSON, small utilities).
//! All execution is blocking (libRhai is sync, and HTTP uses reqwest::blocking),
//! so callers must run these functions inside `spawn_blocking`.

use super::{ExtensionManifest, FieldDef};
use crate::search::ranking::parse_size;
use crate::search::release::normalize_resolution;
use crate::search::{magnet, parse_release, SearchResult, Subtitles};
use crate::subtitles::Subtitle;
use regex::Regex;
use rhai::{Array, Dynamic, Engine, Map as RhaiMap, Scope, AST};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, OnceLock};

const HTTP_TIMEOUT_SECS: u64 = 30;
const HTTP_CONNECT_TIMEOUT_SECS: u64 = 8;
/// how many requests one `http_get_all` call runs at a time
const HTTP_PARALLEL: usize = 8;
/// public trackers added to magnets built from a bare info hash
const DEFAULT_TRACKERS: &[&str] = &[
    "udp://tracker.opentrackr.org:1337/announce",
    "udp://open.stealth.si:80/announce",
    "udp://tracker.torrent.eu.org:451/announce",
    "udp://exodus.desync.com:6969/announce",
    "udp://open.demonii.com:1337/announce",
    "udp://tracker.dler.org:6969/announce",
];
const DEFAULT_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

fn http_client() -> &'static reqwest::blocking::Client {
    static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .user_agent(DEFAULT_USER_AGENT)
            .timeout(std::time::Duration::from_secs(HTTP_TIMEOUT_SECS))
            .connect_timeout(std::time::Duration::from_secs(HTTP_CONNECT_TIMEOUT_SECS))
            .pool_max_idle_per_host(8)
            .build()
            .expect("failed to build http client for extensions")
    })
}

fn cached_regex(pattern: &str) -> Result<Regex, String> {
    static CACHE: OnceLock<Mutex<HashMap<String, Regex>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache.lock().map_err(|e| e.to_string())?;
    if let Some(re) = guard.get(pattern) {
        return Ok(re.clone());
    }
    let re = Regex::new(pattern).map_err(|e| format!("invalid regex '{}': {}", pattern, e))?;
    guard.insert(pattern.to_string(), re.clone());
    Ok(re)
}

type ScriptResult<T> = Result<T, Box<rhai::EvalAltResult>>;

fn http_request(
    method: &str,
    url: &str,
    body: Option<&str>,
    headers: Option<&RhaiMap>,
) -> ScriptResult<String> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(format!("http: invalid url '{}'", url).into());
    }
    let client = http_client();
    let mut req = match method {
        "GET" => client.get(url),
        "POST" => client.post(url),
        _ => return Err(format!("http: unsupported method '{}'", method).into()),
    };
    if let Some(headers) = headers {
        for (k, v) in headers.iter() {
            req = req.header(k.as_str(), v.to_string());
        }
    }
    if let Some(body) = body {
        req = req.body(body.to_string());
    }
    let resp = req
        .send()
        .map_err(|e| format!("http: request to {} failed: {}", url, e))?;
    let status = resp.status();
    let text = resp
        .text()
        .map_err(|e| format!("http: failed to read body from {}: {}", url, e))?;
    if !status.is_success() {
        return Err(format!("http: {} returned status {}", url, status).into());
    }
    Ok(text)
}

fn dynamic_to_i64(value: &Dynamic, default: i64) -> i64 {
    if let Ok(i) = value.as_int() {
        return i;
    }
    if let Ok(f) = value.as_float() {
        return f as i64;
    }
    if let Some(s) = value.read_lock::<rhai::ImmutableString>() {
        return s.trim().parse::<i64>().unwrap_or(default);
    }
    default
}

fn format_size_impl(bytes: i64) -> String {
    let bytes = bytes.max(0) as f64;
    if bytes >= 1_073_741_824.0 {
        format!("{:.2} GB", bytes / 1_073_741_824.0)
    } else if bytes >= 1_048_576.0 {
        format!("{:.2} MB", bytes / 1_048_576.0)
    } else {
        format!("{:.2} KB", bytes / 1024.0)
    }
}

fn html_unescape_impl(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
}

/// fetches every url at once (a few at a time), in order. a failed request
/// gives an empty string, so one dead detail page can't sink a whole search.
fn http_get_all_impl(urls: Array, headers: Option<&RhaiMap>) -> Array {
    let urls: Vec<String> = urls.into_iter().map(|u| u.to_string()).collect();
    let mut bodies: Vec<String> = vec![String::new(); urls.len()];
    for (chunk_urls, chunk_bodies) in urls.chunks(HTTP_PARALLEL).zip(bodies.chunks_mut(HTTP_PARALLEL)) {
        std::thread::scope(|scope| {
            for (url, body) in chunk_urls.iter().zip(chunk_bodies.iter_mut()) {
                scope.spawn(move || match http_request("GET", url, None, headers) {
                    Ok(text) => *body = text,
                    Err(e) => log::info!("[extension] {}", e),
                });
            }
        });
    }
    bodies.into_iter().map(Dynamic::from).collect()
}

pub(crate) fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// unix seconds for the dates trackers print: rfc 2822 ("Sat, 17 Oct 2026
/// 15:42:41 -0000"), iso 8601 ("2026-10-17 15:42:41" or with a t), or a bare number.
pub fn parse_date_impl(text: &str) -> Option<i64> {
    let text = text.trim();
    if let Ok(n) = text.parse::<i64>() {
        return Some(n);
    }
    static RFC: OnceLock<Regex> = OnceLock::new();
    static ISO: OnceLock<Regex> = OnceLock::new();
    let rfc = RFC.get_or_init(|| {
        Regex::new(r"(?i)(\d{1,2})\s+(jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec)\w*\s+(\d{4})(?:\s+(\d{1,2}):(\d{2})(?::(\d{2}))?)?(?:\s*([+-])(\d{2}):?(\d{2}))?").unwrap()
    });
    let iso = ISO.get_or_init(|| {
        Regex::new(r"(\d{4})-(\d{2})-(\d{2})(?:[T\s](\d{2}):(\d{2})(?::(\d{2}))?)?(?:\s*(?:Z|([+-])(\d{2}):?(\d{2})))?").unwrap()
    });
    let num = |c: &regex::Captures, i: usize| c.get(i).and_then(|m| m.as_str().parse::<i64>().ok()).unwrap_or(0);
    let (y, mo, d, c) = if let Some(c) = rfc.captures(text) {
        const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
        let mo = MONTHS.iter().position(|m| c[2].eq_ignore_ascii_case(m))? as i64 + 1;
        (num(&c, 3), mo, num(&c, 1), c)
    } else {
        let c = iso.captures(text)?;
        (num(&c, 1), num(&c, 2), num(&c, 3), c)
    };
    let mut secs = days_from_civil(y, mo, d) * 86400 + num(&c, 4) * 3600 + num(&c, 5) * 60 + num(&c, 6);
    if let Some(sign) = c.get(7) {
        let offset = num(&c, 8) * 3600 + num(&c, 9) * 60;
        secs += if sign.as_str() == "-" { offset } else { -offset };
    }
    Some(secs)
}

/// a magnet for a bare info hash, named and with public trackers.
pub fn magnet_from_hash(hash: &str, name: &str) -> String {
    let mut link = format!("magnet:?xt=urn:btih:{}", hash.trim());
    if !name.is_empty() {
        link.push_str("&dn=");
        link.push_str(&urlencoding::encode(name));
    }
    for tracker in DEFAULT_TRACKERS {
        link.push_str("&tr=");
        link.push_str(&urlencoding::encode(tracker));
    }
    link
}

/// one engine for every script; building it registers the whole host api.
fn shared_engine() -> &'static Engine {
    static ENGINE: OnceLock<Engine> = OnceLock::new();
    ENGINE.get_or_init(build_engine)
}

/// scripts compile once per source text, not on every search.
fn compiled(source: &str) -> Result<Arc<AST>, String> {
    static CACHE: OnceLock<Mutex<HashMap<u64, Arc<AST>>>> = OnceLock::new();
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    let key = hasher.finish();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(ast) = cache.lock().map_err(|e| e.to_string())?.get(&key) {
        return Ok(ast.clone());
    }
    let ast = Arc::new(
        shared_engine()
            .compile(source)
            .map_err(|e| format!("script compile error: {}", e))?,
    );
    cache.lock().map_err(|e| e.to_string())?.insert(key, ast.clone());
    Ok(ast)
}

/// Build a sandboxed engine with the extension host API registered.
pub fn build_engine() -> Engine {
    let mut engine = Engine::new();

    // Guard against runaway scripts. HTTP calls count as single operations,
    // so this only bounds script-side compute.
    engine.set_max_operations(50_000_000);
    engine.set_max_expr_depths(256, 256);
    engine.set_max_call_levels(64);
    engine.set_max_string_size(32 * 1024 * 1024);
    engine.set_max_array_size(200_000);
    engine.set_max_map_size(50_000);

    engine.on_print(|s| log::info!("[extension] {}", s));
    engine.on_debug(|s, _, pos| log::debug!("[extension] {} @ {:?}", s, pos));

    // ── HTTP ──────────────────────────────────────────────────────────
    engine.register_fn("http_get", |url: &str| http_request("GET", url, None, None));
    engine.register_fn("http_get", |url: &str, headers: RhaiMap| {
        http_request("GET", url, None, Some(&headers))
    });
    engine.register_fn("http_get_all", |urls: Array| http_get_all_impl(urls, None));
    engine.register_fn("http_get_all", |urls: Array, headers: RhaiMap| {
        http_get_all_impl(urls, Some(&headers))
    });
    engine.register_fn("http_post", |url: &str, body: &str| {
        http_request("POST", url, Some(body), None)
    });
    engine.register_fn("http_post", |url: &str, body: &str, headers: RhaiMap| {
        http_request("POST", url, Some(body), Some(&headers))
    });

    // ── JSON ──────────────────────────────────────────────────────────
    engine.register_fn("json_parse", |s: &str| -> ScriptResult<Dynamic> {
        let value: serde_json::Value =
            serde_json::from_str(s).map_err(|e| format!("json_parse: {}", e))?;
        rhai::serde::to_dynamic(value).map_err(|e| format!("json_parse: {}", e).into())
    });
    engine.register_fn("json_stringify", |d: Dynamic| -> ScriptResult<String> {
        let value: serde_json::Value =
            rhai::serde::from_dynamic(&d).map_err(|e| format!("json_stringify: {}", e))?;
        serde_json::to_string(&value).map_err(|e| format!("json_stringify: {}", e).into())
    });

    // ── Regex ─────────────────────────────────────────────────────────
    engine.register_fn("regex_matches", |pattern: &str, text: &str| -> ScriptResult<bool> {
        Ok(cached_regex(pattern)?.is_match(text))
    });
    // First match: array of [whole, group1, group2, ...]; empty array if no match.
    engine.register_fn("regex_capture", |pattern: &str, text: &str| -> ScriptResult<Array> {
        let re = cached_regex(pattern)?;
        let mut out = Array::new();
        if let Some(caps) = re.captures(text) {
            for i in 0..caps.len() {
                out.push(caps.get(i).map(|m| m.as_str()).unwrap_or("").into());
            }
        }
        Ok(out)
    });
    // All matches: array of capture arrays.
    engine.register_fn(
        "regex_capture_all",
        |pattern: &str, text: &str| -> ScriptResult<Array> {
            let re = cached_regex(pattern)?;
            let mut out = Array::new();
            for caps in re.captures_iter(text).take(2000) {
                let mut groups = Array::new();
                for i in 0..caps.len() {
                    groups.push(caps.get(i).map(|m| m.as_str()).unwrap_or("").into());
                }
                out.push(groups.into());
            }
            Ok(out)
        },
    );
    // Convenience: first capture group of the first match ("" if none).
    engine.register_fn("regex_group", |pattern: &str, text: &str| -> ScriptResult<String> {
        let re = cached_regex(pattern)?;
        Ok(re
            .captures(text)
            .and_then(|c| c.get(1).or_else(|| c.get(0)))
            .map(|m| m.as_str().to_string())
            .unwrap_or_default())
    });
    engine.register_fn(
        "regex_replace_all",
        |pattern: &str, text: &str, rep: &str| -> ScriptResult<String> {
            Ok(cached_regex(pattern)?.replace_all(text, rep).into_owned())
        },
    );

    // ── Utilities ─────────────────────────────────────────────────────
    engine.register_fn("url_encode", |s: &str| {
        urlencoding::encode(s).into_owned()
    });
    engine.register_fn("html_unescape", |s: &str| html_unescape_impl(s));
    engine.register_fn("format_size", |bytes: i64| format_size_impl(bytes));
    engine.register_fn("parse_int_or", |value: Dynamic, default: i64| {
        dynamic_to_i64(&value, default)
    });
    engine.register_fn("log", |s: &str| log::info!("[extension] {}", s));
    // Bounded sleep — debrid extensions poll their service while a torrent is
    // being cached. Capped so a script can't hang the blocking thread pool.
    engine.register_fn("sleep_ms", |ms: i64| {
        let capped = ms.clamp(0, 15_000) as u64;
        std::thread::sleep(std::time::Duration::from_millis(capped));
    });

    engine.register_fn("parse_size", |s: &str| parse_size(s) as i64);
    engine.register_fn("parse_date", |s: &str| match parse_date_impl(s) {
        Some(secs) => Dynamic::from_int(secs),
        None => Dynamic::UNIT,
    });
    engine.register_fn("info_hash", |magnet: &str| {
        opt_string_to_dynamic(magnet::extract_info_hash(magnet))
    });
    engine.register_fn("magnet_from_hash", |hash: &str, name: &str| magnet_from_hash(hash, name));
    engine.register_fn("magnet_from_hash", |hash: &str| magnet_from_hash(hash, ""));

    // the same release parser magnolia runs on every result, for scripts that
    // want to filter on it. results are parsed anyway, so scripts needn't call it.
    engine.register_fn("parse_release", |title: &str| -> Dynamic {
        rhai::serde::to_dynamic(parse_release(title)).unwrap_or(Dynamic::UNIT)
    });
    // the older, smaller form of parse_release
    engine.register_fn("parse_title_meta", |title: &str| -> RhaiMap {
        let info = parse_release(title);
        let mut map = RhaiMap::new();
        map.insert("season".into(), opt_u32_to_dynamic(info.season));
        map.insert("episode".into(), opt_u32_to_dynamic(info.episode));
        map.insert("quality".into(), opt_string_to_dynamic(info.resolution));
        map.insert("encode".into(), opt_string_to_dynamic(info.video_codec));
        map.insert("is_batch".into(), info.is_batch.into());
        map.insert("audio_codec".into(), opt_string_to_dynamic(info.audio_codec));
        map
    });

    engine
}

fn opt_u32_to_dynamic(v: Option<u32>) -> Dynamic {
    match v {
        Some(v) => Dynamic::from_int(v as i64),
        None => Dynamic::UNIT,
    }
}

fn opt_string_to_dynamic(v: Option<String>) -> Dynamic {
    match v {
        Some(v) => v.into(),
        None => Dynamic::UNIT,
    }
}

/// Run `manifest()` from a script and validate the result.
pub fn parse_manifest(source: &str) -> Result<ExtensionManifest, String> {
    let ast = compiled(source)?;
    let mut scope = Scope::new();
    let result: Dynamic = shared_engine()
        .call_fn(&mut scope, &ast, "manifest", ())
        .map_err(|e| format!("failed to call manifest(): {}", e))?;
    let manifest: ExtensionManifest = rhai::serde::from_dynamic(&result)
        .map_err(|e| format!("invalid manifest: {}", e))?;
    manifest.validate()?;
    Ok(manifest)
}

/// Convert stored field values to typed Rhai values according to the field
/// definitions (checkbox → bool, number → int, everything else → string).
fn build_fields_map(defs: &[FieldDef], values: &HashMap<String, String>) -> RhaiMap {
    let mut map = RhaiMap::new();
    for def in defs {
        let raw = values
            .get(&def.key)
            .cloned()
            .or_else(|| def.default.as_ref().map(json_value_to_string))
            .unwrap_or_default();
        let value: Dynamic = match def.field_type.as_str() {
            "checkbox" => (raw == "true").into(),
            "number" => raw.trim().parse::<i64>().unwrap_or(0).into(),
            _ => raw.into(),
        };
        map.insert(def.key.as_str().into(), value);
    }
    map
}

pub fn json_value_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

pub struct TrackerSearchContext {
    /// the query for this pass, ready to send
    pub query: String,
    /// which pass this is: "episode", "season", "show" or "movie"
    pub kind: String,
    /// the bare show or movie name, for trackers that build their own queries
    pub title: String,
    /// where the episode falls in anilist's numbering, for anime
    pub anime: Option<crate::anime_db::AnimeEpisode>,
    pub year: Option<u32>,
    pub media_type: Option<String>,
    pub imdb_id: Option<String>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct RawSearchResult {
    title: String,
    #[serde(default)]
    magnet_link: Option<String>,
    /// enough on its own: magnolia builds the magnet
    #[serde(default)]
    info_hash: Option<String>,
    #[serde(default)]
    size: Option<String>,
    #[serde(default)]
    size_bytes: Option<i64>,
    #[serde(default)]
    seeds: Option<i64>,
    #[serde(default)]
    peers: Option<i64>,
    #[serde(default)]
    season: Option<i64>,
    #[serde(default)]
    episode: Option<i64>,
    #[serde(default)]
    quality: Option<String>,
    #[serde(default)]
    encode: Option<String>,
    #[serde(default)]
    is_batch: Option<bool>,
    #[serde(default)]
    audio_codec: Option<String>,
    /// "english", "multi", "foreign", "hardcoded" or "none"
    #[serde(default)]
    subtitles: Option<String>,
    #[serde(default)]
    languages: Option<Vec<String>>,
    #[serde(default)]
    hdr: Option<Vec<String>>,
    #[serde(default)]
    group: Option<String>,
    /// unix seconds, or any date `parse_date` reads
    #[serde(default)]
    uploaded: Option<Dynamic>,
    #[serde(default)]
    file_count: Option<i64>,
    #[serde(default)]
    trusted: Option<bool>,
}

fn call_entry_point(
    source: &str,
    fn_name: &str,
    ctx_map: RhaiMap,
) -> Result<Array, String> {
    let ast = compiled(source)?;
    let mut scope = Scope::new();
    let result: Dynamic = shared_engine()
        .call_fn(&mut scope, &ast, fn_name, (Dynamic::from_map(ctx_map),))
        .map_err(|e| format!("{}() failed: {}", fn_name, e))?;
    result
        .into_array()
        .map_err(|t| format!("{}() must return an array, got {}", fn_name, t))
}

/// Execute a tracker extension's `search(ctx)`. Blocking — call from
/// `spawn_blocking`.
pub fn run_tracker_search(
    source: &str,
    manifest: &ExtensionManifest,
    field_values: &HashMap<String, String>,
    ctx: &TrackerSearchContext,
) -> Result<Vec<SearchResult>, String> {
    let provider = manifest
        .tracker_name
        .clone()
        .unwrap_or_else(|| manifest.name.clone());

    let mut ctx_map = RhaiMap::new();
    ctx_map.insert("query".into(), ctx.query.clone().into());
    ctx_map.insert("kind".into(), ctx.kind.clone().into());
    ctx_map.insert("title".into(), ctx.title.clone().into());
    ctx_map.insert("year".into(), opt_u32_to_dynamic(ctx.year));
    ctx_map.insert(
        "anime".into(),
        ctx.anime
            .as_ref()
            .and_then(|a| rhai::serde::to_dynamic(a).ok())
            .unwrap_or(Dynamic::UNIT),
    );
    ctx_map.insert(
        "media_type".into(),
        opt_string_to_dynamic(ctx.media_type.clone()),
    );
    ctx_map.insert("imdb_id".into(), opt_string_to_dynamic(ctx.imdb_id.clone()));
    ctx_map.insert("season".into(), opt_u32_to_dynamic(ctx.season));
    ctx_map.insert("episode".into(), opt_u32_to_dynamic(ctx.episode));
    ctx_map.insert(
        "fields".into(),
        Dynamic::from_map(build_fields_map(&manifest.fields, field_values)),
    );

    let array = call_entry_point(source, "search", ctx_map)?;

    let mut results = Vec::new();
    for item in array {
        let raw: RawSearchResult = match rhai::serde::from_dynamic(&item) {
            Ok(r) => r,
            Err(e) => {
                log::warn!("[extension {}] skipping invalid search result: {}", provider, e);
                continue;
            }
        };
        if let Some(result) = search_result_from_raw(raw, &provider) {
            results.push(result);
        }
    }
    Ok(results)
}

/// a script's result with everything it left out read from the title.
fn search_result_from_raw(raw: RawSearchResult, provider: &str) -> Option<SearchResult> {
    let title = html_unescape_impl(raw.title.trim());
    if title.is_empty() {
        return None;
    }
    let hash = raw
        .info_hash
        .map(|h| h.trim().to_lowercase())
        .filter(|h| h.len() >= 32 && h.chars().all(|c| c.is_ascii_alphanumeric()));
    let magnet_link = match (raw.magnet_link.filter(|m| m.starts_with("magnet:")), &hash) {
        (Some(link), _) => link,
        (None, Some(hash)) => magnet_from_hash(hash, &title),
        (None, None) => return None,
    };
    let info_hash = hash.or_else(|| magnet::extract_info_hash(&magnet_link));

    let mut release = parse_release(&title);
    let positive = |v: Option<i64>| v.filter(|v| *v > 0).map(|v| v as u32);
    if let Some(season) = positive(raw.season) {
        release.season = Some(season);
    }
    if let Some(episode) = positive(raw.episode) {
        release.episode = Some(episode);
    }
    if let Some(is_batch) = raw.is_batch {
        release.is_batch = is_batch;
    }
    if let Some(resolution) = raw.quality.as_deref().and_then(normalize_resolution) {
        release.resolution = Some(resolution);
    }
    release.video_codec = release.video_codec.or(raw.encode);
    release.audio_codec = release.audio_codec.or(raw.audio_codec);
    if let Some(subs) = raw.subtitles.as_deref().and_then(Subtitles::parse) {
        // a tracker's own category beats a guess from the name, except burned in subtitles
        if release.subtitles != Some(Subtitles::Hardcoded) {
            release.subtitles = Some(subs);
        }
    }
    for lang in raw.languages.unwrap_or_default() {
        if !release.languages.contains(&lang) {
            release.languages.push(lang);
        }
    }
    if let Some(hdr) = raw.hdr.filter(|h| !h.is_empty()) {
        release.hdr = hdr;
    }
    if raw.group.is_some() {
        release.group = raw.group;
    }

    let size_bytes = raw.size_bytes.filter(|b| *b > 0).map(|b| b as u64);
    let size = match (raw.size.filter(|s| !s.trim().is_empty()), size_bytes) {
        (Some(size), _) => size,
        (None, Some(bytes)) => format_size_impl(bytes as i64),
        (None, None) => "Unknown".to_string(),
    };
    let uploaded = raw.uploaded.and_then(|d| {
        if let Ok(n) = d.as_int() {
            return Some(n);
        }
        parse_date_impl(&d.into_string().ok()?)
    });

    Some(SearchResult {
        size_bytes: size_bytes.unwrap_or_else(|| parse_size(&size)),
        size,
        seeds: raw.seeds.unwrap_or(0).max(0) as u32,
        peers: raw.peers.unwrap_or(0).max(0) as u32,
        magnet_link,
        info_hash,
        provider: provider.to_string(),
        providers: vec![provider.to_string()],
        uploaded: uploaded.filter(|t| *t > 0),
        file_count: positive(raw.file_count),
        trusted: raw.trusted.unwrap_or(false),
        release,
        title,
        ..Default::default()
    })
}

pub struct SubtitleFetchContext {
    pub tmdb_id: String,
    pub media_type: String,
    pub season: Option<u32>,
    pub episode: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct RawSubtitle {
    url: String,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    lang: Option<String>,
    #[serde(default)]
    display: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    release_name: Option<String>,
    #[serde(default)]
    is_hearing_impaired: Option<bool>,
}

/// Execute a subtitle extension's `fetch_subtitles(ctx)`. Blocking — call
/// from `spawn_blocking`.
pub fn run_subtitle_fetch(
    ext_id: &str,
    source: &str,
    manifest: &ExtensionManifest,
    field_values: &HashMap<String, String>,
    ctx: &SubtitleFetchContext,
) -> Result<Vec<Subtitle>, String> {
    let source_name = manifest
        .subtitle_source_name
        .clone()
        .unwrap_or_else(|| manifest.name.clone());

    let mut ctx_map = RhaiMap::new();
    ctx_map.insert("tmdb_id".into(), ctx.tmdb_id.clone().into());
    ctx_map.insert("media_type".into(), ctx.media_type.clone().into());
    ctx_map.insert("season".into(), opt_u32_to_dynamic(ctx.season));
    ctx_map.insert("episode".into(), opt_u32_to_dynamic(ctx.episode));
    ctx_map.insert(
        "fields".into(),
        Dynamic::from_map(build_fields_map(&manifest.fields, field_values)),
    );

    let array = call_entry_point(source, "fetch_subtitles", ctx_map)?;

    let mut results = Vec::new();
    for (i, item) in array.into_iter().enumerate() {
        let raw: RawSubtitle = match rhai::serde::from_dynamic(&item) {
            Ok(r) => r,
            Err(e) => {
                log::warn!("[extension {}] skipping invalid subtitle: {}", ext_id, e);
                continue;
            }
        };
        if raw.url.is_empty() {
            continue;
        }
        let lang = raw.lang.or_else(|| raw.language.clone()).unwrap_or_default();
        let language = raw.language.unwrap_or_else(|| lang.clone());
        let hi = raw.is_hearing_impaired.unwrap_or(false);
        let display = raw.display.unwrap_or_else(|| {
            if language.is_empty() {
                lang.clone()
            } else {
                language.clone()
            }
        });
        let name = raw.name.unwrap_or_else(|| {
            if hi {
                format!("{} (HI)", display)
            } else {
                display.clone()
            }
        });
        results.push(Subtitle {
            id: raw.id.unwrap_or_else(|| format!("{}-{}", ext_id, i)),
            language,
            lang,
            display,
            url: raw.url,
            is_hearing_impaired: hi,
            source: source_name.clone(),
            name,
            release_name: raw.release_name,
        });
    }
    Ok(results)
}

fn debrid_service_name(manifest: &ExtensionManifest) -> String {
    manifest
        .debrid_name
        .clone()
        .unwrap_or_else(|| manifest.name.clone())
}

pub struct DebridListContext {
    pub magnet: String,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub media_type: Option<String>,
}

/// A file inside a debrid torrent, as reported by the extension's
/// `list_files(ctx)`. Shaped to feed Magnolia's existing (torrent) file
/// selector: `index` is the id the extension wants back in `resolve(ctx)`.
#[derive(Debug, Clone, Serialize)]
pub struct DebridFile {
    pub index: i64,
    pub name: String,
    pub size: u64,
    pub path: String,
}

#[derive(Debug, Deserialize)]
struct RawDebridFile {
    id: i64,
    name: String,
    #[serde(default)]
    size: Option<i64>,
    #[serde(default)]
    path: Option<String>,
}

/// Execute a debrid extension's `list_files(ctx)`: given a magnet, return the
/// files in that torrent. Magnolia runs its own automatic/manual file selection
/// over this list — the script must NOT pick a file itself. Blocking — call
/// from `spawn_blocking`.
pub fn run_debrid_list_files(
    source: &str,
    manifest: &ExtensionManifest,
    field_values: &HashMap<String, String>,
    ctx: &DebridListContext,
) -> Result<Vec<DebridFile>, String> {
    let service = debrid_service_name(manifest);

    let mut ctx_map = RhaiMap::new();
    ctx_map.insert("magnet".into(), ctx.magnet.clone().into());
    ctx_map.insert("season".into(), opt_u32_to_dynamic(ctx.season));
    ctx_map.insert("episode".into(), opt_u32_to_dynamic(ctx.episode));
    ctx_map.insert(
        "media_type".into(),
        opt_string_to_dynamic(ctx.media_type.clone()),
    );
    ctx_map.insert(
        "fields".into(),
        Dynamic::from_map(build_fields_map(&manifest.fields, field_values)),
    );

    let array = call_entry_point(source, "list_files", ctx_map)
        .map_err(|e| format!("[{}] {}", service, e))?;

    let mut files = Vec::new();
    for item in array {
        let raw: RawDebridFile = match rhai::serde::from_dynamic(&item) {
            Ok(r) => r,
            Err(e) => {
                log::warn!("[debrid {}] skipping invalid file entry: {}", service, e);
                continue;
            }
        };
        if raw.name.is_empty() {
            continue;
        }
        let path = raw.path.unwrap_or_else(|| raw.name.clone());
        files.push(DebridFile {
            index: raw.id,
            size: raw.size.unwrap_or(0).max(0) as u64,
            name: raw.name,
            path,
        });
    }
    Ok(files)
}

pub struct DebridResolveContext {
    pub magnet: String,
    /// The id (from `list_files`) of the file Magnolia's selection chose.
    pub file_id: Option<i64>,
    pub file_name: Option<String>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    pub media_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawDebridStream {
    url: String,
}

/// Execute a debrid extension's `resolve(ctx)`, which turns the file Magnolia
/// already selected (`ctx.file_id`) into a directly-playable HTTP URL. The
/// script may return either a bare URL string or a map `#{ url: "..." }`.
/// Blocking — call from `spawn_blocking`.
pub fn run_debrid_resolve(
    source: &str,
    manifest: &ExtensionManifest,
    field_values: &HashMap<String, String>,
    ctx: &DebridResolveContext,
) -> Result<String, String> {
    let service = debrid_service_name(manifest);

    let mut ctx_map = RhaiMap::new();
    ctx_map.insert("magnet".into(), ctx.magnet.clone().into());
    ctx_map.insert(
        "file_id".into(),
        match ctx.file_id {
            Some(i) => Dynamic::from_int(i),
            None => Dynamic::UNIT,
        },
    );
    ctx_map.insert("file_name".into(), opt_string_to_dynamic(ctx.file_name.clone()));
    ctx_map.insert("season".into(), opt_u32_to_dynamic(ctx.season));
    ctx_map.insert("episode".into(), opt_u32_to_dynamic(ctx.episode));
    ctx_map.insert(
        "media_type".into(),
        opt_string_to_dynamic(ctx.media_type.clone()),
    );
    ctx_map.insert(
        "fields".into(),
        Dynamic::from_map(build_fields_map(&manifest.fields, field_values)),
    );

    let ast = compiled(source)?;
    let mut scope = Scope::new();
    let result: Dynamic = shared_engine()
        .call_fn(&mut scope, &ast, "resolve", (Dynamic::from_map(ctx_map),))
        .map_err(|e| format!("[{}] resolve() failed: {}", service, e))?;

    // Accept a bare URL string or a map with a `url` field.
    let url = if result.is_string() {
        result.into_string().unwrap_or_default()
    } else {
        let raw: RawDebridStream = rhai::serde::from_dynamic(&result)
            .map_err(|e| format!("[{}] resolve() must return a url string or #{{ url }}: {}", service, e))?;
        raw.url
    };

    let url = url.trim().to_string();
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(format!(
            "[{}] resolve() returned an invalid stream url: '{}'",
            service, url
        ));
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_parse_in_tracker_formats() {
        assert_eq!(parse_date_impl("Sat, 17 Oct 2026 15:42:41 -0000"), Some(1792251761));
        assert_eq!(parse_date_impl("17 Oct 2026 17:42:41 +0200"), Some(1792251761));
        assert_eq!(parse_date_impl("2026-10-17 15:42:41"), Some(1792251761));
        assert_eq!(parse_date_impl("1774118449"), Some(1774118449));
        assert_eq!(parse_date_impl("yesterday"), None);
    }

    #[test]
    fn results_from_a_bare_info_hash_get_a_magnet() {
        let raw: RawSearchResult = serde_json::from_value(serde_json::json!({
            "title": "Show S01E02 1080p",
            "info_hash": "0123456789ABCDEF0123456789ABCDEF01234567",
            "size_bytes": 1048576,
            "seeds": 3,
            "subtitles": "english",
            "file_count": 1,
            "trusted": true,
        }))
        .unwrap();
        let r = search_result_from_raw(raw, "Test").unwrap();
        assert!(r.magnet_link.starts_with("magnet:?xt=urn:btih:0123456789abcdef"));
        assert_eq!(r.info_hash.as_deref(), Some("0123456789abcdef0123456789abcdef01234567"));
        assert_eq!(r.release.episode, Some(2));
        assert_eq!(r.release.subtitles, Some(Subtitles::English));
        assert_eq!(r.size, "1.00 MB");
        assert_eq!(r.providers, vec!["Test"]);
    }
}
