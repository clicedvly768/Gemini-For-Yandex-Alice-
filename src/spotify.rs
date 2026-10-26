use std::sync::Arc;
use std::time::{Duration, Instant};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::info;

#[derive(Clone)]
pub struct SpotifyClient {
    client: Client,
    client_id: String,
    client_secret: String,
    redirect_uri: String,
    refresh_token: Arc<RwLock<Option<String>>>,
    access_token_cache: Arc<RwLock<Option<(String, Instant)>>>,
    config_file_path: Option<String>,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: u64,
}

#[derive(Deserialize)]
struct CurrentlyPlayingResponse {
    #[allow(dead_code)]
    is_playing: Option<bool>,
    item: Option<SpotifyTrackItem>,
}

#[derive(Deserialize, Clone)]
struct SpotifyTrackItem {
    id: Option<String>,
    name: String,
    uri: Option<String>,
    artists: Vec<SpotifyArtistItem>,
}

#[derive(Deserialize, Clone)]
struct SpotifyArtistItem {
    name: String,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct SearchResponse {
    tracks: Option<SearchTracksResult>,
    playlists: Option<SearchPlaylistsResult>,
    artists: Option<SearchArtistsResult>,
}

#[derive(Deserialize)]
struct SearchTracksResult {
    items: Vec<SpotifyTrackItem>,
}

#[derive(Deserialize)]
struct SearchPlaylistsResult {
    items: Vec<SearchPlaylistItem>,
}

#[derive(Deserialize)]
struct SearchPlaylistItem {
    name: String,
    uri: String,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct SearchArtistsResult {
    items: Vec<SearchArtistDetail>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct SearchArtistDetail {
    name: String,
    uri: String,
}

#[derive(Deserialize)]
struct DevicesResponse {
    devices: Vec<SpotifyDevice>,
}

#[derive(Deserialize, Clone)]
#[allow(dead_code)]
pub struct SpotifyDevice {
    pub id: Option<String>,
    pub name: String,
    #[serde(rename = "type")]
    pub device_type: String,
    pub is_active: bool,
    pub volume_percent: Option<u32>,
}

impl SpotifyClient {
    pub fn new(
        client_id: String,
        client_secret: String,
        redirect_uri: String,
        refresh_token: Arc<RwLock<Option<String>>>,
        config_file_path: Option<String>,
    ) -> Self {
        Self {
            client: Client::new(),
            client_id,
            client_secret,
            redirect_uri,
            refresh_token,
            access_token_cache: Arc::new(RwLock::new(None)),
            config_file_path,
        }
    }

    pub fn is_configured(&self) -> bool {
        !self.client_id.trim().is_empty() && !self.client_secret.trim().is_empty()
    }

    pub async fn is_authorized(&self) -> bool {
        self.refresh_token.read().await.is_some()
    }

    pub fn get_authorize_url(&self) -> String {
        let scopes = "user-modify-playback-state user-read-playback-state user-read-currently-playing user-library-read user-library-modify";
        format!(
            "https://accounts.spotify.com/authorize?response_type=code&client_id={}&scope={}&redirect_uri={}",
            urlencoding::encode(&self.client_id),
            urlencoding::encode(scopes),
            urlencoding::encode(&self.redirect_uri)
        )
    }

    pub async fn handle_callback(&self, code: &str) -> Result<String, String> {
        let basic_auth = BASE64.encode(format!("{}:{}", self.client_id, self.client_secret));

        let params = [
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", &self.redirect_uri),
        ];

        let res = self
            .client
            .post("https://accounts.spotify.com/api/token")
            .header("Authorization", format!("Basic {basic_auth}"))
            .form(&params)
            .send()
            .await
            .map_err(|e| format!("Ошибка запроса токена: {e}"))?;

        if !res.status().is_success() {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            return Err(format!("Spotify ответил ошибкой при получении токена: {status} - {body}"));
        }

        let token_resp: TokenResponse = res
            .json()
            .await
            .map_err(|e| format!("Ошибка парсинга токена: {e}"))?;

        if let Some(ref rt) = token_resp.refresh_token {
            let mut lock = self.refresh_token.write().await;
            *lock = Some(rt.clone());
            let _ = std::fs::write(".spotify_token", rt);

            // Also persist in config.json if configured
            if let Some(ref path_str) = self.config_file_path {
                if let Ok(content) = std::fs::read_to_string(path_str) {
                    if let Ok(mut fc) = serde_json::from_str::<crate::config::FileConfig>(&content) {
                        fc.spotify.refresh_token = Some(rt.clone());
                        if let Ok(serialized) = serde_json::to_string_pretty(&fc) {
                            let _ = std::fs::write(path_str, serialized);
                        }
                    }
                }
            }

            info!("Новый Spotify refresh_token сохранен");
        }

        let mut cache = self.access_token_cache.write().await;
        *cache = Some((
            token_resp.access_token,
            Instant::now() + Duration::from_secs(token_resp.expires_in.saturating_sub(60)),
        ));

        Ok("Spotify успешно авторизован! Теперь вам доступны все функции Google Home / Nest для Spotify.".to_string())
    }

    async fn get_access_token(&self) -> Result<String, String> {
        {
            let cache = self.access_token_cache.read().await;
            if let Some((token, expiry)) = &*cache {
                if Instant::now() < *expiry {
                    return Ok(token.clone());
                }
            }
        }

        let refresh_token = {
            let lock = self.refresh_token.read().await;
            lock.clone().ok_or_else(|| {
                "Spotify не авторизован. Задайте refresh_token в config.json или откройте /auth/login в браузере.".to_string()
            })?
        };

        let basic_auth = BASE64.encode(format!("{}:{}", self.client_id, self.client_secret));
        let params = [
            ("grant_type", "refresh_token"),
            ("refresh_token", &refresh_token),
        ];

        let res = self
            .client
            .post("https://accounts.spotify.com/api/token")
            .header("Authorization", format!("Basic {basic_auth}"))
            .form(&params)
            .send()
            .await
            .map_err(|e| format!("Ошибка обновления токена: {e}"))?;

        if !res.status().is_success() {
            let err_body = res.text().await.unwrap_or_default();
            return Err(format!("Не удалось обновить токен Spotify: {err_body}"));
        }

        let token_resp: TokenResponse = res
            .json()
            .await
            .map_err(|e| format!("Ошибка разбора ответа токена: {e}"))?;

        if let Some(ref new_rt) = token_resp.refresh_token {
            let mut lock = self.refresh_token.write().await;
            *lock = Some(new_rt.clone());
            let _ = std::fs::write(".spotify_token", new_rt);
        }

        let mut cache = self.access_token_cache.write().await;
        *cache = Some((
            token_resp.access_token.clone(),
            Instant::now() + Duration::from_secs(token_resp.expires_in.saturating_sub(60)),
        ));

        Ok(token_resp.access_token)
    }

    pub async fn play(&self) -> Result<String, String> {
        let token = self.get_access_token().await?;
        let res = self
            .client
            .put("https://api.spotify.com/v1/me/player/play")
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| format!("Ошибка сети: {e}"))?;

        match res.status() {
            StatusCode::NO_CONTENT | StatusCode::OK => Ok("Включаю музыку.".to_string()),
            StatusCode::NOT_FOUND => Err("Активное устройство Spotify не найдено. Запустите Spotify на любом устройстве.".to_string()),
            status => {
                let text = res.text().await.unwrap_or_default();
                Err(format!("Ошибка Spotify ({status}): {text}"))
            }
        }
    }

    pub async fn pause(&self) -> Result<String, String> {
        let token = self.get_access_token().await?;
        let res = self
            .client
            .put("https://api.spotify.com/v1/me/player/pause")
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| format!("Ошибка сети: {e}"))?;

        match res.status() {
            StatusCode::NO_CONTENT | StatusCode::OK => Ok("Музыка поставлена на паузу.".to_string()),
            StatusCode::NOT_FOUND => Err("Активное устройство Spotify не найдено.".to_string()),
            status => {
                let text = res.text().await.unwrap_or_default();
                Err(format!("Ошибка Spotify ({status}): {text}"))
            }
        }
    }

    pub async fn next(&self) -> Result<String, String> {
        let token = self.get_access_token().await?;
        let res = self
            .client
            .post("https://api.spotify.com/v1/me/player/next")
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| format!("Ошибка сети: {e}"))?;

        match res.status() {
            StatusCode::NO_CONTENT | StatusCode::OK => Ok("Переключаю на следующий трек.".to_string()),
            status => Err(format!("Ошибка переключения трека: {status}")),
        }
    }

    pub async fn previous(&self) -> Result<String, String> {
        let token = self.get_access_token().await?;
        let res = self
            .client
            .post("https://api.spotify.com/v1/me/player/previous")
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| format!("Ошибка сети: {e}"))?;

        match res.status() {
            StatusCode::NO_CONTENT | StatusCode::OK => Ok("Возвращаю предыдущий трек.".to_string()),
            status => Err(format!("Ошибка переключения на предыдущий трек: {status}")),
        }
    }

    pub async fn set_volume(&self, percent: u32) -> Result<String, String> {
        let percent = percent.min(100);
        let token = self.get_access_token().await?;
        let res = self
            .client
            .put(format!("https://api.spotify.com/v1/me/player/volume?volume_percent={percent}"))
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| format!("Ошибка сети: {e}"))?;

        match res.status() {
            StatusCode::NO_CONTENT | StatusCode::OK => Ok(format!("Громкость установлена на {percent}%")),
            status => Err(format!("Не удалось изменить громкость: {status}")),
        }
    }

    pub async fn currently_playing(&self) -> Result<String, String> {
        let token = self.get_access_token().await?;
        let res = self
            .client
            .get("https://api.spotify.com/v1/me/player/currently-playing")
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| format!("Ошибка сети: {e}"))?;

        if res.status() == StatusCode::NO_CONTENT {
            return Ok("Сейчас ничего не воспроизводится.".to_string());
        }

        if !res.status().is_success() {
            return Err("Не удалось получить информацию о текущем треке.".to_string());
        }

        let info: CurrentlyPlayingResponse = res.json().await.map_err(|e| format!("Ошибка парсинга: {e}"))?;

        if let Some(item) = info.item {
            let artists: Vec<String> = item.artists.into_iter().map(|a| a.name).collect();
            let artists_str = artists.join(", ");
            Ok(format!("Сейчас играет: {} — {}", item.name, artists_str))
        } else {
            Ok("Сейчас ничего не играет.".to_string())
        }
    }

    pub async fn like_current_track(&self) -> Result<String, String> {
        let token = self.get_access_token().await?;

        let res = self
            .client
            .get("https://api.spotify.com/v1/me/player/currently-playing")
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| format!("Ошибка сети: {e}"))?;

        if res.status() == StatusCode::NO_CONTENT || !res.status().is_success() {
            return Err("Сейчас ничего не играет, чтобы поставить лайк.".to_string());
        }

        let info: CurrentlyPlayingResponse = res.json().await.map_err(|e| format!("Ошибка разбора: {e}"))?;
        let item = info.item.ok_or_else(|| "Текущий трек не определен.".to_string())?;
        let track_id = item.id.ok_or_else(|| "ID трека недоступен.".to_string())?;

        let put_res = self
            .client
            .put(format!("https://api.spotify.com/v1/me/tracks?ids={track_id}"))
            .bearer_auth(&token)
            .header("Content-Length", 0)
            .send()
            .await
            .map_err(|e| format!("Ошибка сети: {e}"))?;

        if put_res.status().is_success() {
            let artists: Vec<String> = item.artists.into_iter().map(|a| a.name).collect();
            Ok(format!("Трек «{} — {}» добавлен в Любимые песни!", item.name, artists.join(", ")))
        } else {
            Err("Не удалось добавить трек в медиатеку Spotify.".to_string())
        }
    }

    pub async fn set_shuffle(&self, state: bool) -> Result<String, String> {
        let token = self.get_access_token().await?;
        let res = self
            .client
            .put(format!("https://api.spotify.com/v1/me/player/shuffle?state={state}"))
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| format!("Ошибка сети: {e}"))?;

        if res.status().is_success() {
            if state {
                Ok("Режим случайного воспроизведения (Shuffle) включен.".to_string())
            } else {
                Ok("Режим случайного воспроизведения выключен.".to_string())
            }
        } else {
            Err("Не удалось изменить режим перемешивания.".to_string())
        }
    }

    pub async fn set_repeat(&self, mode: &str) -> Result<String, String> {
        let token = self.get_access_token().await?;
        let res = self
            .client
            .put(format!("https://api.spotify.com/v1/me/player/repeat?state={mode}"))
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| format!("Ошибка сети: {e}"))?;

        if res.status().is_success() {
            let text = match mode {
                "track" => "Повтор текущего трека включен.",
                "context" => "Повтор всего плейлиста включен.",
                _ => "Повтор выключен.",
            };
            Ok(text.to_string())
        } else {
            Err("Не удалось переключить режим повтора.".to_string())
        }
    }

    pub async fn transfer_playback_to(&self, target_name: &str) -> Result<String, String> {
        let token = self.get_access_token().await?;

        let res = self
            .client
            .get("https://api.spotify.com/v1/me/player/devices")
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| format!("Ошибка сети: {e}"))?;

        if !res.status().is_success() {
            return Err("Не удалось получить список активных устройств Spotify.".to_string());
        }

        let dev_resp: DevicesResponse = res.json().await.map_err(|e| format!("Ошибка разбора: {e}"))?;

        let target_lower = target_name.to_lowercase();
        let found = dev_resp.devices.into_iter().find(|d| {
            let d_name = d.name.to_lowercase();
            let d_type = d.device_type.to_lowercase();
            d_name.contains(&target_lower) || d_type.contains(&target_lower)
        });

        let target_device = match found {
            Some(d) => d,
            None => {
                return Err(format!(
                    "Устройство с именем или типом '{target_name}' не найдено в Spotify Connect."
                ));
            }
        };

        let device_id = target_device.id.ok_or_else(|| "ID устройства недоступен.".to_string())?;

        #[derive(Serialize)]
        struct TransferBody {
            device_ids: Vec<String>,
            play: bool,
        }

        let body = TransferBody {
            device_ids: vec![device_id],
            play: true,
        };

        let put_res = self
            .client
            .put("https://api.spotify.com/v1/me/player")
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Ошибка сети: {e}"))?;

        if put_res.status().is_success() {
            Ok(format!("Музыка переключена на устройство «{}».", target_device.name))
        } else {
            Err("Не удалось перевести воспроизведение на выбранное устройство.".to_string())
        }
    }

    pub async fn search_and_play(&self, query: &str) -> Result<String, String> {
        let token = self.get_access_token().await?;

        let is_playlist_query = query.contains("плейлист") || query.contains("подборку");
        let clean_q = query.replace("плейлист", "").replace("подборку", "").trim().to_string();

        let search_url = format!(
            "https://api.spotify.com/v1/search?q={}&type=track,playlist,artist&limit=1",
            urlencoding::encode(if clean_q.is_empty() { query } else { &clean_q })
        );

        let res = self
            .client
            .get(&search_url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| format!("Ошибка поиска Spotify: {e}"))?;

        if !res.status().is_success() {
            return Err("Ошибка при поиске трека в Spotify.".to_string());
        }

        let search_res: SearchResponse = res.json().await.map_err(|e| format!("Ошибка разбора поиска: {e}"))?;

        #[derive(Serialize)]
        struct PlayBody {
            #[serde(skip_serializing_if = "Option::is_none")]
            context_uri: Option<String>,
            #[serde(skip_serializing_if = "Option::is_none")]
            uris: Option<Vec<String>>,
        }

        if is_playlist_query {
            if let Some(playlist) = search_res.playlists.and_then(|p| p.items.into_iter().next()) {
                let play_body = PlayBody {
                    context_uri: Some(playlist.uri),
                    uris: None,
                };
                let play_res = self
                    .client
                    .put("https://api.spotify.com/v1/me/player/play")
                    .bearer_auth(&token)
                    .json(&play_body)
                    .send()
                    .await
                    .map_err(|e| format!("Ошибка запроса: {e}"))?;

                if play_res.status().is_success() {
                    return Ok(format!("Включаю плейлист «{}».", playlist.name));
                }
            }
        }

        if let Some(track) = search_res.tracks.and_then(|t| t.items.into_iter().next()) {
            let artist_names: Vec<String> = track.artists.into_iter().map(|a| a.name).collect();
            let artist_str = artist_names.join(", ");
            let track_name = track.name;
            let track_uri = track.uri.unwrap_or_default();

            let play_body = PlayBody {
                context_uri: None,
                uris: Some(vec![track_uri]),
            };

            let play_res = self
                .client
                .put("https://api.spotify.com/v1/me/player/play")
                .bearer_auth(&token)
                .json(&play_body)
                .send()
                .await
                .map_err(|e| format!("Ошибка запроса воспроизведения: {e}"))?;

            match play_res.status() {
                StatusCode::NO_CONTENT | StatusCode::OK => {
                    return Ok(format!("Включаю трек {track_name} — {artist_str}."));
                }
                StatusCode::NOT_FOUND => {
                    return Err("Активное устройство Spotify не найдено. Откройте приложение Spotify на любом устройстве.".to_string());
                }
                status => {
                    let err_text = play_res.text().await.unwrap_or_default();
                    return Err(format!("Spotify вернул ошибку ({status}): {err_text}"));
                }
            }
        }

        Err(format!("По запросу '{query}' ничего не найдено в Spotify."))
    }
}
