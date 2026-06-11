use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::Read;

const SUBDL_API_URL: &str = "https://api.subdl.com/api/v1/subtitles";
const SUBDL_DL_BASE: &str = "https://dl.subdl.com";

#[derive(Debug, Deserialize)]
struct SubDLResult {
    #[allow(dead_code)]
    sd_id: Option<serde_json::Value>,
    name: Option<String>,
    language: Option<String>,
    lang: Option<String>,
    url: Option<String>,
    subtitles_url: Option<String>,
    hi: Option<bool>,
    #[allow(dead_code)]
    full_season: Option<bool>,
    #[allow(dead_code)]
    author: Option<String>,
    release_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SubDLResponse {
    #[allow(dead_code)]
    status: bool,
    results: Option<Vec<SubDLResult>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Subtitle {
    pub id: String,
    pub language: String,
    pub lang: String,
    pub display: String,
    pub url: String,
    pub is_hearing_impaired: bool,
    pub source: String,
    pub name: String,
    pub release_name: Option<String>,
}

#[tauri::command]
pub async fn fetch_subtitles(
    tmdb_id: String,
    media_type: String,
    season: Option<u32>,
    episode: Option<u32>,
) -> Result<Vec<Subtitle>, String> {
    let client = reqwest::Client::new();

    let media_type_param = if media_type == "tv" { "tv" } else { "movie" };

    let mut query: Vec<(&str, String)> = vec![
        ("tmdb_id", tmdb_id.clone()),
        ("type", media_type_param.to_string()),
    ];

    if media_type == "tv" {
        if let (Some(s), Some(e)) = (season, episode) {
            query.push(("season_number", s.to_string()));
            query.push(("episode_number", e.to_string()));
        }
    }

    let response = client
        .get(SUBDL_API_URL)
        .query(&query)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| e.to_string())?;

    if !status.is_success() {
        return Err(format!("SubDL API error {}: {}", status, body.chars().take(200).collect::<String>()));
    }

    let parsed: SubDLResponse = serde_json::from_str(&body)
        .map_err(|e| format!("SubDL parse error: {} — body: {}", e, body.chars().take(200).collect::<String>()))?;

    let results = parsed.results.unwrap_or_default();

    let mut seen: HashSet<String> = HashSet::new();
    let unique: Vec<Subtitle> = results
        .into_iter()
        .filter_map(|r| {
            let lang = r.lang.as_deref().unwrap_or("").to_string();
            let language = r.language.as_deref().unwrap_or("").to_string();
            let hi = r.hi.unwrap_or(false);
            let download_url = r
                .subtitles_url
                .or_else(|| r.url.as_deref().map(|u| format!("{}{}", SUBDL_DL_BASE, u)))?;

            let key = format!("{}-{}", lang, hi);
            if !seen.insert(key) {
                return None;
            }

            let display = if language.is_empty() { lang.clone() } else { language.clone() };
            let name = if hi {
                format!("{} (HI)", display)
            } else {
                display.clone()
            };

            Some(Subtitle {
                id: format!("subdl-{}", lang),
                language: language.clone(),
                lang,
                display,
                url: download_url,
                is_hearing_impaired: hi,
                source: "subdl".to_string(),
                name,
                release_name: r.release_name,
            })
        })
        .collect();

    Ok(unique)
}

fn extension_from_name(name: &str) -> Option<&'static str> {
    let lower = name.to_lowercase();
    // strip query string for URLs
    let lower = lower.split('?').next().unwrap_or(&lower);
    if lower.ends_with(".ass") || lower.ends_with(".ssa") {
        Some("ass")
    } else if lower.ends_with(".vtt") {
        Some("vtt")
    } else if lower.ends_with(".sub") {
        Some("sub")
    } else if lower.ends_with(".srt") {
        Some("srt")
    } else {
        None
    }
}

#[tauri::command]
pub async fn download_subtitle(url: String) -> Result<String, String> {
    let bytes = reqwest::get(&url)
        .await
        .map_err(|e| format!("Download failed: {}", e))?
        .bytes()
        .await
        .map_err(|e| format!("Read failed: {}", e))?;

    let mut subtitle_content: Option<Vec<u8>> = None;
    let mut subtitle_ext = "srt".to_string();

    if bytes.starts_with(b"PK") {
        // ZIP archive — extract the first subtitle file inside
        let cursor = std::io::Cursor::new(bytes);
        let mut archive =
            zip::ZipArchive::new(cursor).map_err(|e| format!("ZIP open failed: {}", e))?;

        for i in 0..archive.len() {
            let mut file = archive
                .by_index(i)
                .map_err(|e| format!("ZIP read failed: {}", e))?;
            if let Some(ext) = extension_from_name(file.name()) {
                subtitle_ext = ext.to_string();
                let mut content = Vec::new();
                file.read_to_end(&mut content)
                    .map_err(|e| format!("Read sub file failed: {}", e))?;
                subtitle_content = Some(content);
                break;
            }
        }
    } else {
        // Raw subtitle file (extension subtitle sources may link directly)
        subtitle_ext = extension_from_name(&url).unwrap_or("srt").to_string();
        subtitle_content = Some(bytes.to_vec());
    }

    let content = subtitle_content
        .ok_or_else(|| "No subtitle file found in ZIP archive".to_string())?;

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let path = std::env::temp_dir().join(format!("magnolia_sub_{}.{}", ts, subtitle_ext));
    std::fs::write(&path, &content).map_err(|e| format!("Write temp failed: {}", e))?;

    Ok(path.to_string_lossy().to_string())
}
