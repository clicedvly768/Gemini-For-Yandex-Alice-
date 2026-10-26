use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::{error, info};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String, // "user" or "model"
    pub text: String,
}

struct SessionContext {
    messages: Vec<ChatMessage>,
    last_active: Instant,
}

#[derive(Clone)]
pub struct GeminiClient {
    client: Client,
    api_key: String,
    model: String,
    history: Arc<Mutex<HashMap<String, SessionContext>>>,
}

#[derive(Serialize)]
struct GeminiPart {
    text: String,
}

#[derive(Serialize)]
struct GeminiContent {
    role: String,
    parts: Vec<GeminiPart>,
}

#[derive(Serialize)]
struct GeminiSystemInstruction {
    parts: Vec<GeminiPart>,
}

#[derive(Serialize)]
struct GeminiRequest {
    #[serde(rename = "systemInstruction", skip_serializing_if = "Option::is_none")]
    system_instruction: Option<GeminiSystemInstruction>,
    contents: Vec<GeminiContent>,
}

#[derive(Deserialize)]
struct GeminiResponseCandidate {
    content: Option<GeminiResponseContent>,
}

#[derive(Deserialize)]
struct GeminiResponseContent {
    parts: Option<Vec<GeminiResponsePart>>,
}

#[derive(Deserialize)]
struct GeminiResponsePart {
    text: Option<String>,
}

#[derive(Deserialize)]
struct GeminiResponse {
    candidates: Option<Vec<GeminiResponseCandidate>>,
    error: Option<GeminiError>,
}

#[derive(Deserialize, Debug)]
struct GeminiError {
    message: String,
}

impl GeminiClient {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            model,
            history: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn ask(&self, session_id: &str, user_query: &str) -> Result<String, String> {
        if self.api_key.trim().is_empty() {
            return Err("Ключ GEMINI_API_KEY не задан в конфигурации.".to_string());
        }

        // Clean up old sessions (> 30 mins)
        {
            let mut hist = self.history.lock().await;
            hist.retain(|_, v| v.last_active.elapsed().as_secs() < 1800);
        }

        // Retrieve conversation history
        let mut previous_messages = {
            let hist = self.history.lock().await;
            hist.get(session_id)
                .map(|ctx| ctx.messages.clone())
                .unwrap_or_default()
        };

        // Prepare request contents
        let mut contents = Vec::new();
        for msg in &previous_messages {
            contents.push(GeminiContent {
                role: msg.role.clone(),
                parts: vec![GeminiPart {
                    text: msg.text.clone(),
                }],
            });
        }

        // Add current user prompt
        contents.push(GeminiContent {
            role: "user".to_string(),
            parts: vec![GeminiPart {
                text: user_query.to_string(),
            }],
        });

        // Google Nest Smart Persona Prompt
        let system_prompt = "Ты — голосовой ассистент следующего поколения (Google Nest / Gemini Live), \
работающий в умной колонке. \
Твоя манера общения: живая, дружелюбная, тактичная, естественная и лаконичная. \
Отвечай на чистом русском языке. \
Ответ должен быть приспособлен для синтеза речи (TTS): \
- Не используй Markdown (звездочки, решетки, жирный шрифт, списки, таблицы), так как колонка читает текст вслух; \
- Держи ответ емким (2-4 предложения), не перегружай лишними деталями, если тебя прямо не попросили рассказать подробно; \
- Если пользователь просит рецепт, совет или объяснение, давай самое главное без воды; \
- Если пользователь спрашивает про таймеры, погоду или утреннюю сводку (Good Morning Routine) — отвечай структурированно и тепло.";

        let req_body = GeminiRequest {
            system_instruction: Some(GeminiSystemInstruction {
                parts: vec![GeminiPart {
                    text: system_prompt.to_string(),
                }],
            }),
            contents,
        };

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.model, self.api_key
        );

        let response = self
            .client
            .post(&url)
            .json(&req_body)
            .send()
            .await
            .map_err(|e| format!("Ошибка запроса к Gemini: {e}"))?;

        if !response.status().is_success() {
            let status = response.status();
            let err_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Неизвестная ошибка".to_string());
            error!("Gemini API error ({status}): {err_text}");
            return Err(format!("Gemini ответил с ошибкой: {status}"));
        }

        let resp_json: GeminiResponse = response
            .json()
            .await
            .map_err(|e| format!("Ошибка разбора ответа Gemini: {e}"))?;

        if let Some(err) = resp_json.error {
            return Err(format!("Gemini API вернул ошибку: {}", err.message));
        }

        let raw_answer = resp_json
            .candidates
            .as_ref()
            .and_then(|c| c.first())
            .and_then(|c| c.content.as_ref())
            .and_then(|c| c.parts.as_ref())
            .and_then(|p| p.first())
            .and_then(|p| p.text.as_ref())
            .cloned()
            .unwrap_or_else(|| "Извините, не удалось сформировать ответ.".to_string());

        let clean_answer = clean_text_for_alice(&raw_answer);

        // Update session history
        previous_messages.push(ChatMessage {
            role: "user".to_string(),
            text: user_query.to_string(),
        });
        previous_messages.push(ChatMessage {
            role: "model".to_string(),
            text: clean_answer.clone(),
        });

        // Limit history per session to last 10 messages
        if previous_messages.len() > 10 {
            let excess = previous_messages.len() - 10;
            previous_messages.drain(0..excess);
        }

        {
            let mut hist = self.history.lock().await;
            hist.insert(
                session_id.to_string(),
                SessionContext {
                    messages: previous_messages,
                    last_active: Instant::now(),
                },
            );
        }

        info!("Gemini response generated successfully for session {}", session_id);
        Ok(clean_answer)
    }
}

pub fn clean_text_for_alice(text: &str) -> String {
    let mut cleaned = text
        .replace("**", "")
        .replace('*', "")
        .replace("###", "")
        .replace("##", "")
        .replace('#', "")
        .replace("```", "")
        .replace('`', "")
        .replace('_', "")
        .replace('~', "")
        .replace(">", "")
        .trim()
        .to_string();

    // Alice hard limit is 1024 chars
    if cleaned.chars().count() > 950 {
        let mut char_count = 0;
        let mut cut_idx = 0;
        let chars: Vec<(usize, char)> = cleaned.char_indices().collect();

        for (idx, ch) in &chars {
            if char_count >= 900 && (*ch == '.' || *ch == '!' || *ch == '?') {
                cut_idx = *idx + ch.len_utf8();
                break;
            }
            char_count += 1;
            if char_count >= 940 {
                cut_idx = *idx;
                break;
            }
        }

        if cut_idx > 0 && cut_idx < cleaned.len() {
            cleaned.truncate(cut_idx);
            cleaned.push_str("...");
        }
    }

    cleaned
}
