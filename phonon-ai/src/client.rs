use crate::AiError;
use reqwest::Client;
use std::time::Duration;

/// A credential whose `Debug` output is redacted, so providers can keep
/// deriving `Debug` without leaking keys into logs.
#[derive(Clone)]
pub(crate) struct ApiKey(String);

impl ApiKey {
  pub(crate) fn new(key: impl Into<String>) -> Self {
    Self(key.into())
  }

  /// The raw key, for building request headers only.
  pub(crate) fn expose(&self) -> &str {
    &self.0
  }
}

impl std::fmt::Debug for ApiKey {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_str("ApiKey(<redacted>)")
  }
}

/// A thin HTTP client wrapper for TTS API calls.
///
/// Manages the underlying reqwest client, base URL, API key, and timeout settings.
#[derive(Debug, Clone)]
pub struct HttpClient {
  inner: Client,
  base_url: String,
  api_key: ApiKey,
}

impl HttpClient {
  /// Creates a new `HttpClient`.
  ///
  /// # Arguments
  ///
  /// * `api_key` - The API key for authentication.
  /// * `base_url` - The base URL of the API (e.g. "https://api.openai.com/v1").
  /// * `timeout_secs` - Request timeout in seconds. Default: 120.
  pub fn new(
    api_key: impl Into<String>,
    base_url: impl Into<String>,
    timeout_secs: Option<u64>,
  ) -> Result<Self, AiError> {
    let timeout = Duration::from_secs(timeout_secs.unwrap_or(120));
    let inner = Client::builder()
      .timeout(timeout)
      .build()
      .map_err(|e| AiError::Http(e.to_string()))?;

    Ok(Self {
      inner,
      base_url: base_url.into(),
      api_key: ApiKey::new(api_key),
    })
  }

  /// Creates a client configured for OpenAI TTS.
  ///
  /// Reads the API key from the `OPENAI_API_KEY` environment variable.
  pub fn openai() -> Result<Self, AiError> {
    let api_key = std::env::var("OPENAI_API_KEY")
      .map_err(|_| AiError::Config("OPENAI_API_KEY environment variable not set".to_string()))?;
    Self::new(api_key, "https://api.openai.com/v1", None)
  }

  /// Creates a client configured for an OpenAI-compatible API with custom base URL.
  ///
  /// Reads the API key from the `OPENAI_API_KEY` environment variable.
  pub fn openai_compatible(base_url: impl Into<String>) -> Result<Self, AiError> {
    let api_key = std::env::var("OPENAI_API_KEY")
      .map_err(|_| AiError::Config("OPENAI_API_KEY environment variable not set".to_string()))?;
    Self::new(api_key, base_url, None)
  }

  /// Returns a reference to the inner reqwest client.
  pub fn inner(&self) -> &Client {
    &self.inner
  }

  /// Returns the base URL.
  pub fn base_url(&self) -> &str {
    &self.base_url
  }

  /// Returns the API key.
  pub fn api_key(&self) -> &str {
    self.api_key.expose()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_debug_redacts_api_key() {
    let client = HttpClient::new("sk-very-secret", "https://example.invalid", None).unwrap();
    let debug = format!("{client:?}");
    assert!(!debug.contains("sk-very-secret"), "{debug}");
    assert_eq!(client.api_key(), "sk-very-secret");
  }
}
