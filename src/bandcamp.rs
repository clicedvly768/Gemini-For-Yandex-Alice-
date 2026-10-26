use reqwest::Client;
use serde::Deserialize;
use std::sync::Arc;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::{error, info, warn};

#[derive(Clone)]
pub struct BandcampPlayer {
    client: Client,
    current_process: Arc<Mutex<Option<Child>>>,
    current_title: Arc<Mutex<Option<String>>>,
}

#[derive(Deserialize, Debug)]
struct AutocompleteResponse {
    results: Option<Vec<AutocompleteItem>>,
}

#[derive(Deserialize, Debug)]
struct AutocompleteItem {
    name: String,
    #[serde(rename = "type")]
    item_type: String, // "t" for track, "a" for album, "b" for artist
    url: String,
    band_name: Option<String>,
}

#[derive(Clone, Debug)]
pub struct BandcampTrack {
    pub title: String,
    pub artist: String,
    pub stream_url: String,
    pub page_url: String,
}

impl BandcampPlayer {
    pub fn new() -> Self {
        let client = Client::builder()
            .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .build()
            .unwrap_or_else(|_| Client::new());

        Self {
            client,
            current_process: Arc::new(Mutex::new(None)),
            current_title: Arc::new(Mutex::new(None)),
        }
    }

    /// Search Bandcamp and extract streaming MP3 url
    pub async fn search_track(&self, query: &str) -> Result<BandcampTrack, String> {
        let search_url = format!(
            "https://bandcamp.com/api/fuzzysearch/1/app_autocomplete?q={}",
            urlencoding::encode(query)
        );

        let res = self
            .client
            .get(&search_url)
            .send()
            .await
            .map_err(|e| format!("Ошибка поиска Bandcamp: {e}"))?;

        if !res.status().is_success() {
            return Err(format!("Bandcamp API вернул статус: {}", res.status()));
        }

        let auto_resp: AutocompleteResponse = res
            .json()
            .await
            .map_err(|e| format!("Ошибка разбора ответа поиска: {e}"))?;

        let results = auto_resp.results.ok_or_else(|| "Ничего не найдено.".to_string())?;

        // Prefer track ("t"), then album ("a")
        let item = results
            .iter()
            .find(|i| i.item_type == "t")
            .or_else(|| results.iter().find(|i| i.item_type == "a"))
            .or_else(|| results.first())
            .ok_or_else(|| format!("По запросу «{query}» ничего не найдено."))?;

        // Fix malformed URLs like "https://aphextwin.bandcamp.comhttps://aphextwin.bandcamp.com/track/..."
        let raw_url = &item.url;
        let page_url = if let Some(last_http) = raw_url.rfind("https://") {
            if last_http > 0 {
                raw_url[last_http..].to_string()
            } else {
                raw_url.clone()
            }
        } else if let Some(last_http) = raw_url.rfind("http://") {
            if last_http > 0 {
                raw_url[last_http..].to_string()
            } else {
                raw_url.clone()
            }
        } else {
            raw_url.clone()
        };

        info!("Получена страница Bandcamp: {}", page_url);

        // Fetch page HTML to extract mp3 stream
        let page_html = self
            .client
            .get(&page_url)
            .send()
            .await
            .map_err(|e| format!("Ошибка открытия страницы трека: {e}"))?
            .text()
            .await
            .map_err(|e| format!("Ошибка чтения HTML: {e}"))?;

        let stream_url = extract_mp3_url(&page_html)
            .ok_or_else(|| "Не удалось извлечь поток аудио (возможно, трек доступен только для покупки).".to_string())?;

        let artist = item.band_name.clone().unwrap_or_else(|| "Bandcamp Artist".to_string());
        let title = item.name.clone();

        Ok(BandcampTrack {
            title,
            artist,
            stream_url,
            page_url,
        })
    }

    /// Play stream locally on device using ffplay, mpv or gst-launch
    pub async fn play_query(&self, query: &str) -> Result<String, String> {
        let track = self.search_track(query).await?;
        self.stop().await;

        let stream_url = track.stream_url.clone();
        let display_title = format!("{} — {}", track.title, track.artist);

        // Try playing with ffplay, mpv or gst-launch
        let mut cmd = if is_command_available("ffplay").await {
            let mut c = Command::new("ffplay");
            c.arg("-nodisp")
                .arg("-autoexit")
                .arg("-loglevel")
                .arg("quiet")
                .arg(&stream_url);
            Some(c)
        } else if is_command_available("mpv").await {
            let mut c = Command::new("mpv");
            c.arg("--no-video").arg("--really-quiet").arg(&stream_url);
            Some(c)
        } else if is_command_available("gst-launch-1.0").await {
            let mut c = Command::new("gst-launch-1.0");
            c.arg("playbin").arg(format!("uri={stream_url}"));
            Some(c)
        } else {
            None
        };

        if let Some(ref mut command) = cmd {
            match command.spawn() {
                Ok(child) => {
                    let mut lock = self.current_process.lock().await;
                    *lock = Some(child);
                    let mut t_lock = self.current_title.lock().await;
                    *t_lock = Some(display_title.clone());

                    Ok(format!("Включаю на Bandcamp: {display_title}"))
                }
                Err(e) => {
                    error!("Не удалось запустить аудиоплеер: {e}");
                    Ok(format!(
                        "Найден трек «{display_title}», но локальный медиаплеер не смог запуститься: {e}. Ссылка на поток: {stream_url}"
                    ))
                }
            }
        } else {
            // Player not found on host, return stream link
            let mut t_lock = self.current_title.lock().await;
            *t_lock = Some(display_title.clone());
            Ok(format!(
                "Найден трек на Bandcamp: «{display_title}». Ссылка: {stream_url}"
            ))
        }
    }

    pub async fn stop(&self) -> String {
        let mut lock = self.current_process.lock().await;
        if let Some(mut child) = lock.take() {
            let _ = child.kill().await;
            let mut t_lock = self.current_title.lock().await;
            *t_lock = None;
            "Воспроизведение Bandcamp остановлено.".to_string()
        } else {
            "Сейчас ничего не играет на Bandcamp.".to_string()
        }
    }

    pub async fn currently_playing(&self) -> Option<String> {
        let lock = self.current_title.lock().await;
        lock.clone()
    }
}

async fn is_command_available(name: &str) -> bool {
    tokio::process::Command::new("which")
        .arg(name)
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn extract_mp3_url(html: &str) -> Option<String> {
    // Search for mp3-128 stream link in TralbumData
    // Pattern: "mp3-128":"https:\/\/..." or "mp3-128&quot;:&quot;https:\/\/..."
    let clean_html = html.replace("&quot;", "\"").replace("&amp;", "&").replace("\\/", "/");

    let marker = "\"mp3-128\":\"";
    if let Some(start) = clean_html.find(marker) {
        let after = &clean_html[start + marker.len()..];
        if let Some(end) = after.find('"') {
            let url = &after[..end];
            if url.starts_with("http") {
                return Some(url.to_string());
            }
        }
    }

    // Secondary fallback search
    for line in clean_html.lines() {
        if line.contains("mp3-128") && line.contains("https://") {
            let parts: Vec<&str> = line.split("https://").collect();
            if parts.len() > 1 {
                let candidate = parts[1];
                let end_idx = candidate.find(|c| c == '"' || c == '\'' || c == ' ' || c == '&').unwrap_or(candidate.len());
                return Some(format!("https://{}", &candidate[..end_idx]));
            }
        }
    }

    None
}
