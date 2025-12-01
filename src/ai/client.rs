use crate::error::MetaError;
use serde_json::{json, Value};
use tokio::time::{sleep, Duration};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

const GEMINI_MODEL: &str = "gemini-2.5-flash-preview-09-2025";

pub struct GeminiClient {
    client: reqwest::Client,
    api_key: String,
}

impl GeminiClient {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .unwrap_or_default(),
            api_key: std::env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set"),
        }
    }

    pub async fn generate(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        response_schema: Option<Value>,
        stage_name: &str,
    ) -> Result<String, MetaError> {
        let max_retries = 3;
        
        for attempt in 1..=max_retries {
            match self.generate_attempt(system_prompt, user_prompt, response_schema.clone(), stage_name).await {
                Ok(text) => return Ok(text),
                Err(e) => {
                    log::warn!("Attempt {attempt}/{max_retries} failed: {e}");
                    if attempt == max_retries {
                        return Err(e);
                    }
                    sleep(Duration::from_secs(2u64.pow(attempt as u32))).await;
                }
            }
        }
        Err(MetaError::GenerationFailed("Max retries exceeded".into()))
    }

    async fn generate_attempt(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        response_schema: Option<Value>,
        stage_name: &str,
    ) -> Result<String, MetaError> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            GEMINI_MODEL, self.api_key
        );

        let full_prompt = format!("{system_prompt}\n\n{user_prompt}");

        let mut payload = json!({
            "contents": [{
                "parts": [{ "text": full_prompt }]
            }],
            "generationConfig": {
                "responseMimeType": "application/json"
            }
        });

        if let Some(schema) = response_schema {
            payload["generationConfig"]["responseSchema"] = schema;
        }

        let res = self.client.post(&url).json(&payload).send().await?;

        if !res.status().is_success() {
            let status = res.status();
            let err_text = res.text().await.unwrap_or_default();
            log::error!("API Error: {}", err_text);
            return Err(MetaError::GenerationFailed(format!("API Error {status}: {err_text}")));
        }

        let body: Value = res.json().await?;
        
        let text = body["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .ok_or_else(|| MetaError::GenerationFailed("No text content returned".into()))?;

        let cleaned_text = clean_json_block(text);

        // --- DUMP RESPONSE TO TIMESTAMPED FILE ---
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        // Sanitize stage name
        let safe_stage = stage_name.replace(" ", "_").replace("/", "-");
        let filename = format!("llm_response_{}_{}.json", safe_stage, timestamp);

        if let Err(e) = fs::write(&filename, &cleaned_text) {
            log::warn!("Failed to dump response to {}: {}", filename, e);
        } else {
            log::info!("💾 LLM Response dumped to '{}'", filename);
        }
        // -----------------------------------------

        Ok(cleaned_text)
    }
}

fn clean_json_block(text: &str) -> String {
    let start = text.find("```json").map(|i| i + 7).unwrap_or(0);
    let end = text.rfind("```").unwrap_or(text.len());
    text[start..end].trim().to_string()
}