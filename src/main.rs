mod alice;
mod bandcamp;
mod config;
mod gemini;
mod spotify;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::response::{Html, IntoResponse, Redirect};
use axum::routing::{get, post};
use axum::Router;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

use crate::alice::{alice_webhook_handler, AppState};
use crate::bandcamp::BandcampPlayer;
use crate::config::AppConfig;
use crate::gemini::GeminiClient;
use crate::spotify::SpotifyClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load .env if present
    let _ = dotenvy::dotenv();

    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    let _ = tracing::subscriber::set_global_default(subscriber);

    let config = AppConfig::load();
    info!("Загружена конфигурация. Сервер: {}:{}", config.host, config.port);

    let gemini = GeminiClient::new(config.gemini_api_key.clone(), config.gemini_model.clone());
    let spotify = SpotifyClient::new(
        config.spotify_client_id.clone(),
        config.spotify_client_secret.clone(),
        config.spotify_redirect_uri.clone(),
        config.spotify_refresh_token.clone(),
        config.config_file_path.clone(),
    );
    let bandcamp = BandcampPlayer::new();

    let state = Arc::new(AppState {
        gemini,
        spotify,
        bandcamp,
    });

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/alice", post(alice_webhook_handler))
        .route("/health", get(health_handler))
        .route("/auth/login", get(spotify_login_handler))
        .route("/auth/callback", get(spotify_callback_handler))
        .route("/api/bandcamp/play", get(bandcamp_play_api_handler))
        .route("/api/bandcamp/stop", get(bandcamp_stop_api_handler))
        .with_state(state);

    let addr: SocketAddr = format!("{}:{}", config.host, config.port).parse()?;
    info!("Запуск HTTP сервера на http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn health_handler() -> &'static str {
    "OK"
}

async fn index_handler(State(state): State<Arc<AppState>>) -> Html<String> {
    let spotify_auth = state.spotify.is_authorized().await;
    let auth_badge = if spotify_auth {
        "<span style='color: green; font-weight: bold;'>Подключен</span>"
    } else {
        "<span style='color: red; font-weight: bold;'>Не подключен</span>"
    };

    let bandcamp_status = state.bandcamp.currently_playing().await;
    let bc_badge = match bandcamp_status {
        Some(track) => format!("<span style='color: #1DA0C3; font-weight: bold;'>Играет: {}</span>", track),
        None => "<span style='color: #888;'>Не играет</span>".to_string(),
    };

    let html = format!(
        r#"<!DOCTYPE html>
<html lang="ru">
<head>
    <meta charset="utf-8">
    <title>Gemini + Spotify + Bandcamp Media Center</title>
    <style>
        body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; margin: 40px; background: #f7f9fa; color: #333; }}
        .card {{ background: white; border-radius: 12px; padding: 24px; box-shadow: 0 4px 12px rgba(0,0,0,0.08); max-width: 640px; margin: auto; }}
        h1 {{ margin-top: 0; color: #111; font-size: 24px; }}
        .btn {{ display: inline-block; padding: 10px 18px; border-radius: 8px; text-decoration: none; color: white; background: #1DB954; font-weight: bold; margin-top: 6px; }}
        .btn:hover {{ background: #1aa34a; }}
        .btn-bc {{ background: #1DA0C3; }}
        .btn-bc:hover {{ background: #198ba9; }}
        code {{ background: #f0f0f0; padding: 2px 6px; border-radius: 4px; }}
        ul {{ line-height: 1.6; }}
    </style>
</head>
<body>
    <div class="card">
        <h1>🎵 Gemini + Spotify + Bandcamp</h1>
        <p><b>Статус Spotify:</b> {}</p>
        <p><a href="/auth/login" class="btn">Авторизовать Spotify</a></p>
        <hr style="margin: 20px 0; border: none; border-top: 1px solid #eee;">
        <p><b>Статус Bandcamp:</b> {}</p>
        <p>
            <a href="/api/bandcamp/stop" class="btn btn-bc">Остановить Bandcamp</a>
        </p>
        <hr style="margin: 20px 0; border: none; border-top: 1px solid #eee;">
        <h3>Команды для Алисы:</h3>
        <ul>
            <li><b>Bandcamp:</b> «Включи на Bandcamp Aphex Twin», «Останови Bandcamp», «Что играет на Bandcamp»</li>
            <li><b>Spotify:</b> «Включи Queen», «Лайкни этот трек», «Перемешай треки», «Переключи на компьютер»</li>
            <li><b>Gemini:</b> «Доброе утро» (сводка дня), любые вопросы или рецепты.</li>
        </ul>
    </div>
</body>
</html>"#,
        auth_badge, bc_badge
    );

    Html(html)
}

async fn bandcamp_play_api_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Html<String> {
    let query = params.get("q").cloned().unwrap_or_else(|| "ambient".to_string());
    match state.bandcamp.play_query(&query).await {
        Ok(msg) => Html(format!("<h3>{}</h3><p><a href='/'>Назад</a></p>", msg)),
        Err(e) => Html(format!("<h3>Ошибка: {}</h3><p><a href='/'>Назад</a></p>", e)),
    }
}

async fn bandcamp_stop_api_handler(State(state): State<Arc<AppState>>) -> Html<String> {
    let msg = state.bandcamp.stop().await;
    Html(format!("<h3>{}</h3><p><a href='/'>Назад</a></p>", msg))
}

async fn spotify_login_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    if !state.spotify.is_configured() {
        return Html(
            "<h3>Ошибка: SPOTIFY_CLIENT_ID или SPOTIFY_CLIENT_SECRET не заданы в конфиге</h3>".to_string(),
        )
        .into_response();
    }

    let url = state.spotify.get_authorize_url();
    Redirect::temporary(&url).into_response()
}

async fn spotify_callback_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<HashMap<String, String>>,
) -> Html<String> {
    if let Some(code) = params.get("code") {
        match state.spotify.handle_callback(code).await {
            Ok(msg) => Html(format!(
                "<div style='font-family: sans-serif; text-align: center; margin-top: 50px;'>
                    <h2 style='color: green;'>Успешно!</h2>
                    <p>{}</p>
                    <p><a href='/'>Вернуться на главную</a></p>
                </div>",
                msg
            )),
            Err(err) => Html(format!(
                "<div style='font-family: sans-serif; text-align: center; margin-top: 50px;'>
                    <h2 style='color: red;'>Ошибка авторизации Spotify</h2>
                    <p>{}</p>
                    <p><a href='/'>Назад</a></p>
                </div>",
                err
            )),
        }
    } else if let Some(err) = params.get("error") {
        Html(format!(
            "<div style='font-family: sans-serif; text-align: center; margin-top: 50px;'>
                <h2 style='color: red;'>Spotify отклонил доступ</h2>
                <p>{}</p>
            </div>",
            err
        ))
    } else {
        Html("<h3>Некорректный запрос callback</h3>".to_string())
    }
}
