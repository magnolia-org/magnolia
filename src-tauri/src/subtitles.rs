use serde::{Deserialize, Serialize};
use std::collections::HashSet;

const WYZIE_BASE_URL: &str = "https://wyzie.wyziemagnolia.workers.dev/search";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiSubtitle {
    id: String,
    url: String,
    #[allow(dead_code)]
    format: Option<serde_json::Value>,
    encoding: Option<String>,
    display: String,
    language: String,
    is_hearing_impaired: bool,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ApiResponse {
    Subtitles(Vec<ApiSubtitle>),
    Error { message: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct WyzieSubtitle {
    pub id: String,
    pub language: String,
    pub display: String,
    pub url: String,
    pub format: String,
    pub is_hearing_impaired: bool,
    pub encoding: String,
    pub source: String,
    pub name: String,
}

#[tauri::command]
pub async fn fetch_wyzie_subtitles(
    tmdb_id: String,
    media_type: String,
    season: Option<u32>,
    episode: Option<u32>,
) -> Result<Vec<WyzieSubtitle>, String> {
    let client = reqwest::Client::new();

    // Wyzie API expects IMDb-style IDs: tt{id}
    let formatted_id = if tmdb_id.starts_with("tt") {
        tmdb_id
    } else {
        format!("tt{}", tmdb_id)
    };

    let mut query: Vec<(&str, String)> = vec![
        ("id", formatted_id),
    ];

    if media_type == "tv" {
        if let (Some(s), Some(e)) = (season, episode) {
            query.push(("season", s.to_string()));
            query.push(("episode", e.to_string()));
        }
    }

    let response = client
        .get(WYZIE_BASE_URL)
        .query(&query)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let api_response: ApiResponse = response
        .json()
        .await
        .map_err(|e| e.to_string())?;

    let subtitles = match api_response {
        ApiResponse::Subtitles(list) => list,
        ApiResponse::Error { message } => return Err(message),
    };

    let mut seen: HashSet<String> = HashSet::new();
    let unique: Vec<WyzieSubtitle> = subtitles
        .into_iter()
        .filter(|sub| {
            let key = format!("{}-{}", sub.language, sub.is_hearing_impaired);
            seen.insert(key)
        })
        .map(|sub| {
            let name = if sub.is_hearing_impaired {
                format!("{} (HI)", sub.display)
            } else {
                sub.display.clone()
            };
            WyzieSubtitle {
                id: sub.id,
                language: sub.language,
                display: sub.display,
                url: sub.url,
                format: "srt".to_string(),
                is_hearing_impaired: sub.is_hearing_impaired,
                encoding: sub.encoding.unwrap_or_default(),
                source: "wyzie".to_string(),
                name,
            }
        })
        .collect();

    Ok(unique)
}
