use std::sync::Arc;
use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::bandcamp::BandcampPlayer;
use crate::gemini::GeminiClient;
use crate::spotify::SpotifyClient;

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct AliceRequest {
    pub meta: Option<AliceMeta>,
    pub request: Option<AliceRequestDetails>,
    pub session: AliceSession,
    pub version: String,
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct AliceMeta {
    pub locale: Option<String>,
    pub timezone: Option<String>,
    pub client_id: Option<String>,
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct AliceSession {
    pub session_id: String,
    pub message_id: i64,
    pub skill_id: Option<String>,
    pub user_id: Option<String>,
    pub new: bool,
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct AliceRequestDetails {
    pub command: Option<String>,
    pub original_utterance: Option<String>,
    #[serde(rename = "type")]
    pub utterance_type: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct AliceResponse {
    pub response: AliceResponseDetails,
    pub version: String,
}

#[derive(Serialize, Debug)]
pub struct AliceResponseDetails {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tts: Option<String>,
    pub end_session: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buttons: Option<Vec<AliceButton>>,
}

#[derive(Serialize, Debug)]
pub struct AliceButton {
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub hide: bool,
}

#[derive(Clone)]
pub struct AppState {
    pub gemini: GeminiClient,
    pub spotify: SpotifyClient,
    pub bandcamp: BandcampPlayer,
}

pub async fn alice_webhook_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<AliceRequest>,
) -> Json<AliceResponse> {
    info!(
        session_id = %payload.session.session_id,
        is_new = payload.session.new,
        "Получен запрос от Яндекс Алисы"
    );

    let raw_command = payload
        .request
        .as_ref()
        .and_then(|r| r.command.as_ref())
        .map(|s| s.trim().to_lowercase())
        .unwrap_or_default();

    let original_utterance = payload
        .request
        .as_ref()
        .and_then(|r| r.original_utterance.as_ref())
        .map(|s| s.trim())
        .unwrap_or_default();

    // 1. First interaction in the session
    if payload.session.new && raw_command.is_empty() {
        let welcome = "Привет! Я умный медиацентр с Google Gemini, Spotify и Bandcamp.\n\n\
            Доступные команды:\n\
            • Bandcamp: 'Включи на Bandcamp Aphex Twin', 'Останови Bandcamp'\n\
            • Spotify: 'Включи Queen', 'Перемешай треки', 'Лайкни этот трек', 'Переключи на компьютер'\n\
            • Gemini Live: 'Доброе утро' (сводка дня), советы, рецепты и любые вопросы.\n\
            Что включить или о чем рассказать?";

        return Json(make_response(welcome, false, Some(default_buttons())));
    }

    // Exit commands
    if raw_command == "выход" || (raw_command == "хватит" && raw_command.len() == 12) || raw_command == "пока" {
        return Json(make_response("До свидания! Будет скучно — заходите еще.", true, None));
    }

    // 2. Bandcamp commands
    if let Some(bc_reply) = handle_bandcamp_command(&raw_command, &state.bandcamp).await {
        return Json(make_response(&bc_reply, false, Some(default_buttons())));
    }

    // 3. Google Routine: "Доброе утро" (Good Morning Daily Briefing)
    if raw_command == "доброе утро" || raw_command == "добрый день" || raw_command == "утренняя сводка" {
        let prompt = "Пользователь поздоровался утренней фразой 'Доброе утро'. \
            Поприветствуй его в стиле утренней сводки Google Assistant: пожелай отличного дня, \
            напомни пару бодрящих мыслей или интересный факт дня.";
        let reply = match state.gemini.ask(&payload.session.session_id, prompt).await {
            Ok(ans) => ans,
            Err(e) => format!("Доброе утро! Прекрасного вам дня и отличного настроения! ({e})"),
        };
        return Json(make_response(&reply, false, Some(default_buttons())));
    }

    // 4. Google Routine: "Спокойной ночи" (Bedtime routine / White noise / Sleep)
    if raw_command == "спокойной ночи" || raw_command == "доброй ночи" {
        let reply = "Спокойной ночи! Сладких снов. Если захотите, могу включить расслабляющий плейлист для сна — просто скажите «Включи музыку для сна».";
        return Json(make_response(reply, false, Some(default_buttons())));
    }

    // 5. Check for Spotify controls (including Google Nest unique features)
    if let Some(spotify_reply) = handle_spotify_command(&raw_command, &state.spotify).await {
        return Json(make_response(&spotify_reply, false, Some(default_buttons())));
    }

    // 6. Fallback to Google Gemini
    let query_to_gemini = if !original_utterance.is_empty() {
        original_utterance
    } else {
        &raw_command
    };

    let reply = match state.gemini.ask(&payload.session.session_id, query_to_gemini).await {
        Ok(answer) => answer,
        Err(err) => {
            warn!("Ошибка Gemini: {}", err);
            format!("Не удалось получить ответ от Gemini: {}", err)
        }
    };

    Json(make_response(&reply, false, Some(default_buttons())))
}

async fn handle_bandcamp_command(command: &str, bandcamp: &BandcampPlayer) -> Option<String> {
    let cmd = command.trim();

    // Stop bandcamp
    if cmd == "стоп бэндкэмп" || cmd == "стоп бандкамп" || cmd == "останови бэндкэмп" || cmd == "останови бандкамп" || cmd == "останови bandcamp" || cmd == "стоп bandcamp" {
        return Some(bandcamp.stop().await);
    }

    // What's playing on bandcamp
    if cmd == "что играет на bandcamp" || cmd == "что играет на бэндкэмп" || cmd == "что играет на бандкамп" {
        if let Some(title) = bandcamp.currently_playing().await {
            return Some(format!("Сейчас на Bandcamp играет: {title}"));
        } else {
            return Some("Сейчас на Bandcamp ничего не играет.".to_string());
        }
    }

    // Play on bandcamp
    let prefixes = [
        "включи на bandcamp ",
        "включи на бэндкэмп ",
        "включи на бандкамп ",
        "поставь на bandcamp ",
        "поставь на бэндкэмп ",
        "найди на bandcamp ",
        "найди на бэндкэмп ",
        "найди на бандкамп ",
        "bandcamp ",
        "бэндкэмп ",
        "бандкамп ",
    ];

    for prefix in prefixes {
        if cmd.starts_with(prefix) {
            let query = cmd[prefix.len()..].trim();
            if !query.is_empty() {
                return Some(match bandcamp.play_query(query).await {
                    Ok(msg) => msg,
                    Err(e) => format!("Не удалось включить на Bandcamp: {e}"),
                });
            }
        }
    }

    None
}

async fn handle_spotify_command(command: &str, spotify: &SpotifyClient) -> Option<String> {
    let cmd = command.trim();

    // 1. Google Home feature: Like / Add to favorites ("Лайкни песню", "Мне нравится", "Добавь в любимые")
    if cmd == "мне нравится" || cmd == "лайк" || cmd == "лайкни" || cmd == "лайкни трек" || cmd == "лайкни песню" || cmd == "добавь в любимые" || cmd == "сохрани трек" {
        return Some(match spotify.like_current_track().await {
            Ok(msg) => msg,
            Err(e) => e,
        });
    }

    // 2. Google Home feature: Shuffle ("Перемешай", "Включи перемешивание", "Случайный порядок")
    if cmd == "перемешай" || cmd == "перемешай треки" || cmd == "включи перемешивание" || cmd == "шаффл" || cmd == "shuffle" {
        return Some(match spotify.set_shuffle(true).await {
            Ok(msg) => msg,
            Err(e) => e,
        });
    }
    if cmd == "выключи перемешивание" || cmd == "выключи шаффл" {
        return Some(match spotify.set_shuffle(false).await {
            Ok(msg) => msg,
            Err(e) => e,
        });
    }

    // 3. Google Home feature: Repeat ("Повторяй этот трек", "Зацикли песню")
    if cmd == "повторяй трек" || cmd == "повторяй песню" || cmd == "повтор трека" || cmd == "зацикли" || cmd == "поставь на повтор" {
        return Some(match spotify.set_repeat("track").await {
            Ok(msg) => msg,
            Err(e) => e,
        });
    }
    if cmd == "выключи повтор" || cmd == "отключи повтор" {
        return Some(match spotify.set_repeat("off").await {
            Ok(msg) => msg,
            Err(e) => e,
        });
    }

    // 4. Google Home feature: Transfer playback / Cast ("Переключи музыку на телефон / компьютер")
    let transfer_prefixes = ["переключи на ", "переведи на ", "играй на ", "перенеси на ", "переключи музыку на "];
    for prefix in transfer_prefixes {
        if cmd.starts_with(prefix) {
            let target = cmd[prefix.len()..].trim();
            if !target.is_empty() {
                return Some(match spotify.transfer_playback_to(target).await {
                    Ok(msg) => msg,
                    Err(e) => e,
                });
            }
        }
    }

    // 5. Basic Playback: Pause
    if cmd == "пауза" || cmd == "стоп" || cmd == "останови музыку" || cmd == "останови" || cmd == "паузу" {
        return Some(match spotify.pause().await {
            Ok(msg) => msg,
            Err(e) => e,
        });
    }

    // 6. Resume / Play
    if cmd == "продолжи" || cmd == "играй" || cmd == "возобнови" || cmd == "плей" || cmd == "play" || cmd == "продолжи воспроизведение" {
        return Some(match spotify.play().await {
            Ok(msg) => msg,
            Err(e) => e,
        });
    }

    // 7. Next track
    if cmd == "следующий" || cmd == "следующий трек" || cmd == "дальше" || cmd == "переключи" || cmd == "пропусти" || cmd == "скип" || cmd == "следующая песня" {
        return Some(match spotify.next().await {
            Ok(msg) => msg,
            Err(e) => e,
        });
    }

    // 8. Previous track
    if cmd == "предыдущий" || cmd == "предыдущий трек" || cmd == "назад" || cmd == "предыдущая" || cmd == "верни назад" {
        return Some(match spotify.previous().await {
            Ok(msg) => msg,
            Err(e) => e,
        });
    }

    // 9. Currently playing
    if cmd == "что играет" || cmd == "какой трек" || cmd == "что сейчас играет" || cmd == "текущий трек" || cmd == "кто поет" || cmd == "что за песня" {
        return Some(match spotify.currently_playing().await {
            Ok(msg) => msg,
            Err(e) => e,
        });
    }

    // 10. Volume control
    if cmd.starts_with("громкость") || cmd.starts_with("сделай громкость") {
        let num_str: String = cmd.chars().filter(|c| c.is_ascii_digit()).collect();
        if let Ok(percent) = num_str.parse::<u32>() {
            return Some(match spotify.set_volume(percent).await {
                Ok(msg) => msg,
                Err(e) => e,
            });
        }
    }

    // 11. Search and play: track, playlist, artist, album
    let prefixes = [
        "включи плейлист ", "поставь плейлист ", "включи песню ", "включи трек ",
        "поставь песню ", "поставь трек ", "включи ", "поставь ", "запусти песню ", "найди песню "
    ];
    for prefix in prefixes {
        if cmd.starts_with(prefix) {
            let track_query = cmd[prefix.len()..].trim();
            if !track_query.is_empty() {
                return Some(match spotify.search_and_play(track_query).await {
                    Ok(msg) => msg,
                    Err(e) => e,
                });
            }
        }
    }

    None
}

fn make_response(text: &str, end_session: bool, buttons: Option<Vec<AliceButton>>) -> AliceResponse {
    AliceResponse {
        response: AliceResponseDetails {
            text: text.to_string(),
            tts: None,
            end_session,
            buttons,
        },
        version: "1.0".to_string(),
    }
}

fn default_buttons() -> Vec<AliceButton> {
    vec![
        AliceButton {
            title: "Bandcamp Aphex Twin 🎧".to_string(),
            payload: None,
            url: None,
            hide: true,
        },
        AliceButton {
            title: "Лайкни трек ❤️".to_string(),
            payload: None,
            url: None,
            hide: true,
        },
        AliceButton {
            title: "Перемешай треки 🔀".to_string(),
            payload: None,
            url: None,
            hide: true,
        },
        AliceButton {
            title: "Доброе утро ☀️".to_string(),
            payload: None,
            url: None,
            hide: true,
        },
    ]
}
