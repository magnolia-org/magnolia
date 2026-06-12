//! Rhai engine setup and script execution for extensions.
//!
//! Scripts are sandboxed: they have no filesystem or process access, only the
//! host functions registered here (HTTP, regex, JSON, small utilities).
//! All execution is blocking (libRhai is sync, and HTTP uses reqwest::blocking),
//! so callers must run these functions inside `spawn_blocking`.

use super::{ExtensionManifest, FieldDef};
use crate::search::{parse_audio_codec, parse_title_metadata, SearchResult};
use crate::subtitles::Subtitle;
use regex::Regex;
use rhai::{Array, Dynamic, Engine, Map as RhaiMap, Scope};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

const HTTP_TIMEOUT_SECS: u64 = 30;
const DEFAULT_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

fn http_client() -> &'static reqwest::blocking::Client {
    static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .user_agent(DEFAULT_USER_AGENT)
            .timeout(std::time::Duration::from_secs(HTTP_TIMEOUT_SECS))
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

    // Shared release-title parser so tracker extensions get the same
    // season/episode/quality detection as the built-in providers had.
    engine.register_fn("parse_title_meta", |title: &str| -> RhaiMap {
        let meta = parse_title_metadata(title);
        let mut map = RhaiMap::new();
        map.insert("season".into(), opt_u32_to_dynamic(meta.season));
        map.insert("episode".into(), opt_u32_to_dynamic(meta.episode));
        map.insert("quality".into(), opt_string_to_dynamic(meta.quality));
        map.insert("encode".into(), opt_string_to_dynamic(meta.encode));
        map.insert("is_batch".into(), meta.is_batch.into());
        map.insert(
            "audio_codec".into(),
            opt_string_to_dynamic(parse_audio_codec(title)),
        );
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
    let engine = build_engine();
    let ast = engine
        .compile(source)
        .map_err(|e| format!("script compile error: {}", e))?;
    let mut scope = Scope::new();
    let result: Dynamic = engine
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
    pub query: String,
    pub media_type: Option<String>,
    pub imdb_id: Option<String>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct RawSearchResult {
    title: String,
    magnet_link: String,
    #[serde(default)]
    size: Option<String>,
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
}

fn call_entry_point(
    source: &str,
    fn_name: &str,
    ctx_map: RhaiMap,
) -> Result<Array, String> {
    let engine = build_engine();
    let ast = engine
        .compile(source)
        .map_err(|e| format!("script compile error: {}", e))?;
    let mut scope = Scope::new();
    let result: Dynamic = engine
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
        if raw.title.is_empty() || !raw.magnet_link.starts_with("magnet:") {
            continue;
        }

        // Auto-fill anything the script didn't provide from the title.
        let meta = parse_title_metadata(&raw.title);
        results.push(SearchResult {
            season: raw.season.map(|v| v.max(0) as u32).or(meta.season),
            episode: raw.episode.map(|v| v.max(0) as u32).or(meta.episode),
            quality: raw.quality.or(meta.quality),
            encode: raw.encode.or(meta.encode),
            is_batch: raw.is_batch.unwrap_or(meta.is_batch),
            audio_codec: raw.audio_codec.or_else(|| parse_audio_codec(&raw.title)),
            size: raw.size.unwrap_or_else(|| "Unknown".to_string()),
            seeds: raw.seeds.unwrap_or(0).max(0) as u32,
            peers: raw.peers.unwrap_or(0).max(0) as u32,
            title: raw.title,
            magnet_link: raw.magnet_link,
            provider: provider.clone(),
        });
    }
    Ok(results)
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

    let engine = build_engine();
    let ast = engine
        .compile(source)
        .map_err(|e| format!("script compile error: {}", e))?;
    let mut scope = Scope::new();
    let result: Dynamic = engine
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
