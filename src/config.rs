use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ServerSettings {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
}

fn default_host() -> String {
    "0.0.0.0".to_string()
}
fn default_port() -> u16 {
    8080
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GeminiSettings {
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_gemini_model")]
    pub model: String,
    #[serde(default)]
    pub prompt_override: String,
}

fn default_gemini_model() -> String {
    "gemini-2.0-flash".to_string()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpotifySettings {
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub client_secret: String,
    #[serde(default)]
    pub redirect_uri: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BandcampSettings {
    #[serde(default = "default_bc_enabled")]
    pub enabled: bool,
    #[serde(default = "default_bc_volume")]
    pub default_volume: u32,
}

fn default_bc_enabled() -> bool {
    true
}
fn default_bc_volume() -> u32 {
    80
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileConfig {
    #[serde(default)]
    pub server: ServerSettings,
    #[serde(default)]
    pub gemini: GeminiSettings,
    #[serde(default)]
    pub spotify: SpotifySettings,
    #[serde(default)]
    pub bandcamp: BandcampSettings,
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
        }
    }
}

impl Default for GeminiSettings {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            model: default_gemini_model(),
            prompt_override: String::new(),
        }
    }
}

impl Default for SpotifySettings {
    fn default() -> Self {
        Self {
            client_id: String::new(),
            client_secret: String::new(),
            redirect_uri: String::new(),
            refresh_token: None,
        }
    }
}

impl Default for BandcampSettings {
    fn default() -> Self {
        Self {
            enabled: default_bc_enabled(),
            default_volume: default_bc_volume(),
        }
    }
}

impl Default for FileConfig {
    fn default() -> Self {
        Self {
            server: ServerSettings::default(),
            gemini: GeminiSettings::default(),
            spotify: SpotifySettings::default(),
            bandcamp: BandcampSettings::default(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub host: String,
    pub port: u16,
    pub gemini_api_key: String,
    pub gemini_model: String,
    pub spotify_client_id: String,
    pub spotify_client_secret: String,
    pub spotify_redirect_uri: String,
    pub spotify_refresh_token: Arc<RwLock<Option<String>>>,
    pub bandcamp_enabled: bool,
    pub config_file_path: Option<String>,
}

impl AppConfig {
    pub fn load() -> Self {
        // Priority 1: Check config.json paths (current dir, /etc/gemini-alice/config.json, /tmp/config.json)
        let candidates = [
            "config.json",
            "/tmp/config.json",
            "/etc/gemini-alice/config.json",
        ];

        for path_str in candidates {
            if Path::new(path_str).exists() {
                if let Ok(content) = fs::read_to_string(path_str) {
                    if let Ok(mut fc) = serde_json::from_str::<FileConfig>(&content) {
                        info!("Загружен конфигурационный файл: {}", path_str);

                        // If redirect_uri is empty, generate from port
                        if fc.spotify.redirect_uri.is_empty() {
                            fc.spotify.redirect_uri =
                                format!("http://localhost:{}/auth/callback", fc.server.port);
                        }

                        // Check .spotify_token override
                        if fc.spotify.refresh_token.is_none() || fc.spotify.refresh_token.as_deref() == Some("") {
                            if let Ok(t) = fs::read_to_string(".spotify_token") {
                                let t = t.trim().to_string();
                                if !t.is_empty() {
                                    fc.spotify.refresh_token = Some(t);
                                }
                            }
                        }

                        return Self {
                            host: fc.server.host,
                            port: fc.server.port,
                            gemini_api_key: fc.gemini.api_key,
                            gemini_model: fc.gemini.model,
                            spotify_client_id: fc.spotify.client_id,
                            spotify_client_secret: fc.spotify.client_secret,
                            spotify_redirect_uri: fc.spotify.redirect_uri,
                            spotify_refresh_token: Arc::new(RwLock::new(fc.spotify.refresh_token)),
                            bandcamp_enabled: fc.bandcamp.enabled,
                            config_file_path: Some(path_str.to_string()),
                        };
                    }
                }
            }
        }

        // Priority 2: Fallback to environment variables / .env
        info!("Файл config.json не найден, загрузка из переменных окружения / .env");
        let host = env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
        let port = env::var("PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(8080);

        let gemini_api_key = env::var("GEMINI_API_KEY").unwrap_or_default();
        let gemini_model =
            env::var("GEMINI_MODEL").unwrap_or_else(|_| "gemini-2.0-flash".to_string());

        let spotify_client_id = env::var("SPOTIFY_CLIENT_ID").unwrap_or_default();
        let spotify_client_secret = env::var("SPOTIFY_CLIENT_SECRET").unwrap_or_default();
        let spotify_redirect_uri = env::var("SPOTIFY_REDIRECT_URI")
            .unwrap_or_else(|_| format!("http://localhost:{port}/auth/callback"));

        let mut spotify_refresh_token = env::var("SPOTIFY_REFRESH_TOKEN").ok();
        if spotify_refresh_token.is_none() {
            if let Ok(t) = fs::read_to_string(".spotify_token") {
                let t = t.trim().to_string();
                if !t.is_empty() {
                    spotify_refresh_token = Some(t);
                }
            }
        }

        Self {
            host,
            port,
            gemini_api_key,
            gemini_model,
            spotify_client_id,
            spotify_client_secret,
            spotify_redirect_uri,
            spotify_refresh_token: Arc::new(RwLock::new(spotify_refresh_token)),
            bandcamp_enabled: true,
            config_file_path: None,
        }
    }

    /// Automatically persist refresh token into config.json if available
    pub async fn save_spotify_refresh_token(&self, new_token: &str) {
        let mut lock = self.spotify_refresh_token.write().await;
        *lock = Some(new_token.to_string());

        // Always save to .spotify_token
        let _ = fs::write(".spotify_token", new_token);

        // Also update config.json if it exists
        if let Some(ref path_str) = self.config_file_path {
            if let Ok(content) = fs::read_to_string(path_str) {
                if let Ok(mut fc) = serde_json::from_str::<FileConfig>(&content) {
                    fc.spotify.refresh_token = Some(new_token.to_string());
                    if let Ok(serialized) = serde_json::to_string_pretty(&fc) {
                        let _ = fs::write(path_str, serialized);
                        info!("Обновленный Spotify refresh_token сохранен в {}", path_str);
                    }
                }
            }
        }
    }
}
