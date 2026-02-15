use std::time::Duration;

use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use tokio::time::sleep;

use crate::error::{CoreError, CoreResult};

#[derive(Debug, Clone, uniffi::Record)]
pub struct RetryConfig {
    pub max_retries: u32,
    pub initial_delay_ms: u64,
    pub max_delay_ms: u64,
    pub backoff_factor: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay_ms: 200,
            max_delay_ms: 10_000,
            backoff_factor: 2.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct OpenRouterConfig {
    pub api_key: String,
    pub model_name: String,
    pub base_url: String,
    pub retry: RetryConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, uniffi::Record)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ChoiceMessage,
}

#[derive(Debug, Deserialize)]
struct ChoiceMessage {
    content: String,
}

#[derive(Clone)]
pub struct ModelConnector {
    client: reqwest::Client,
    config: OpenRouterConfig,
}

impl ModelConnector {
    pub fn new(config: OpenRouterConfig) -> CoreResult<Self> {
        if config.api_key.trim().is_empty() {
            return Err(CoreError::Config("OpenRouter API key is empty".to_owned()));
        }
        if config.model_name.trim().is_empty() {
            return Err(CoreError::Config("Model name is empty".to_owned()));
        }

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| CoreError::Model(e.to_string()))?;

        Ok(Self { client, config })
    }

    pub async fn test_connection(&self) -> CoreResult<()> {
        let base = self.config.base_url.trim_end_matches('/');
        let url = format!("{base}/models");

        let response = self
            .request_with_retry(|| {
                self.client
                    .get(&url)
                    .header("Authorization", format!("Bearer {}", self.config.api_key))
            })
            .await?;

        if response.status().is_success() {
            Ok(())
        } else {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            Err(CoreError::Model(format!(
                "model connection failed with status {}: {}",
                status, body
            )))
        }
    }

    pub async fn chat_completion(&self, messages: &[ChatMessage]) -> CoreResult<String> {
        let base = self.config.base_url.trim_end_matches('/');
        let url = format!("{base}/chat/completions");

        let payload = serde_json::json!({
            "model": self.config.model_name,
            "messages": messages,
            "stream": false,
        });

        let response = self
            .request_with_retry(|| {
                self.client
                    .post(&url)
                    .header("Authorization", format!("Bearer {}", self.config.api_key))
                    .header("Content-Type", "application/json")
                    .json(&payload)
            })
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(CoreError::Model(format!(
                "chat completion failed with status {}: {}",
                status, body
            )));
        }

        let body: ChatResponse = response
            .json()
            .await
            .map_err(|e| CoreError::Model(e.to_string()))?;

        let content = body
            .choices
            .first()
            .map(|choice| choice.message.content.clone())
            .ok_or_else(|| CoreError::Model("empty model response".to_owned()))?;

        Ok(content)
    }

    async fn request_with_retry(
        &self,
        mut build_request: impl FnMut() -> reqwest::RequestBuilder,
    ) -> CoreResult<reqwest::Response> {
        let mut attempt: u32 = 0;

        loop {
            let result = build_request().send().await;

            match result {
                Ok(response) => {
                    if response.status().is_success() {
                        return Ok(response);
                    }

                    if !is_retryable_status(response.status()) {
                        return Ok(response);
                    }

                    if attempt >= self.config.retry.max_retries {
                        return Ok(response);
                    }
                }
                Err(err) => {
                    if attempt >= self.config.retry.max_retries || !is_retryable_error(&err) {
                        return Err(CoreError::Model(err.to_string()));
                    }
                }
            }

            let delay_ms = compute_backoff_ms(attempt, &self.config.retry);
            sleep(Duration::from_millis(delay_ms)).await;
            attempt += 1;
        }
    }
}

pub(crate) fn is_retryable_status(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::TOO_MANY_REQUESTS
            | StatusCode::INTERNAL_SERVER_ERROR
            | StatusCode::BAD_GATEWAY
            | StatusCode::SERVICE_UNAVAILABLE
            | StatusCode::GATEWAY_TIMEOUT
    )
}

pub(crate) fn is_retryable_error(err: &reqwest::Error) -> bool {
    err.is_timeout() || err.is_connect() || err.is_request()
}

pub(crate) fn compute_backoff_ms(attempt: u32, config: &RetryConfig) -> u64 {
    let raw = (config.initial_delay_ms as f64) * config.backoff_factor.powf(attempt as f64);
    raw.min(config.max_delay_ms as f64) as u64
}

#[cfg(test)]
mod tests {
    use reqwest::StatusCode;

    use super::{compute_backoff_ms, is_retryable_status, RetryConfig};

    #[test]
    fn retryable_status_is_correct() {
        assert!(is_retryable_status(StatusCode::TOO_MANY_REQUESTS));
        assert!(is_retryable_status(StatusCode::INTERNAL_SERVER_ERROR));
        assert!(is_retryable_status(StatusCode::BAD_GATEWAY));
        assert!(is_retryable_status(StatusCode::SERVICE_UNAVAILABLE));
        assert!(is_retryable_status(StatusCode::GATEWAY_TIMEOUT));

        assert!(!is_retryable_status(StatusCode::UNAUTHORIZED));
        assert!(!is_retryable_status(StatusCode::BAD_REQUEST));
        assert!(!is_retryable_status(StatusCode::NOT_FOUND));
    }

    #[test]
    fn backoff_caps_at_max_delay() {
        let config = RetryConfig {
            max_retries: 5,
            initial_delay_ms: 200,
            max_delay_ms: 1000,
            backoff_factor: 2.0,
        };

        assert_eq!(compute_backoff_ms(0, &config), 200);
        assert_eq!(compute_backoff_ms(1, &config), 400);
        assert_eq!(compute_backoff_ms(2, &config), 800);
        assert_eq!(compute_backoff_ms(3, &config), 1000);
        assert_eq!(compute_backoff_ms(4, &config), 1000);
    }
}
