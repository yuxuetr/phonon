use crate::AiError;
use reqwest::Client;
use std::future::Future;
use std::time::Duration;
use tracing::warn;

/// Attempts per request in [`retry_transient`], including the first one.
const MAX_ATTEMPTS: u32 = 3;
/// First backoff delay in [`retry_transient`]; doubles on each retry.
const RETRY_BASE_DELAY: Duration = Duration::from_millis(500);
/// Upper bound for establishing a connection, separate from the overall
/// request timeout so an unreachable host fails fast.
pub(crate) const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Read a provider credential from the environment variable `name`.
pub(crate) fn env_key(name: &str) -> Result<String, AiError> {
  std::env::var(name).map_err(|_| AiError::Config(format!("{name} environment variable not set")))
}

/// Run `op`, retrying with exponential backoff (3 attempts, 500 ms then
/// 1 s) while it fails with a [transient](AiError::is_transient) error.
///
/// Only wrap requests that are safe to repeat: a retried TTS call costs
/// another synthesis but has no other side effect.
pub async fn retry_transient<T, F, Fut>(op: F) -> Result<T, AiError>
where
  F: FnMut() -> Fut,
  Fut: Future<Output = Result<T, AiError>>,
{
  retry_with_base_delay(RETRY_BASE_DELAY, op).await
}

async fn retry_with_base_delay<T, F, Fut>(base_delay: Duration, mut op: F) -> Result<T, AiError>
where
  F: FnMut() -> Fut,
  Fut: Future<Output = Result<T, AiError>>,
{
  let mut attempt = 1;
  loop {
    match op().await {
      Err(e) if e.is_transient() && attempt < MAX_ATTEMPTS => {
        let delay = base_delay * 2u32.pow(attempt - 1);
        warn!(attempt, ?delay, error = %e, "transient provider error, retrying");
        tokio::time::sleep(delay).await;
        attempt += 1;
      }
      result => return result,
    }
  }
}

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
      .connect_timeout(CONNECT_TIMEOUT)
      .build()
      .map_err(|e| AiError::Http(e.to_string()))?;

    Ok(Self {
      inner,
      base_url: base_url.into(),
      api_key: ApiKey::new(api_key),
    })
  }

  /// Wraps a caller-configured reqwest client, e.g. one with custom proxy,
  /// TLS or timeout settings.
  pub fn with_client(
    inner: Client,
    api_key: impl Into<String>,
    base_url: impl Into<String>,
  ) -> Self {
    Self {
      inner,
      base_url: base_url.into(),
      api_key: ApiKey::new(api_key),
    }
  }

  /// Creates a client configured for OpenAI TTS.
  ///
  /// Reads the API key from the `OPENAI_API_KEY` environment variable.
  pub fn openai() -> Result<Self, AiError> {
    Self::new(
      env_key("OPENAI_API_KEY")?,
      "https://api.openai.com/v1",
      None,
    )
  }

  /// Creates a client configured for an OpenAI-compatible API with custom base URL.
  ///
  /// Reads the API key from the `OPENAI_API_KEY` environment variable.
  pub fn openai_compatible(base_url: impl Into<String>) -> Result<Self, AiError> {
    Self::new(env_key("OPENAI_API_KEY")?, base_url, None)
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

  #[test]
  fn test_env_key_missing_is_config_error() {
    // A name no environment sets, so the test never mutates process state.
    let err = env_key("PHONON_TEST_DEFINITELY_UNSET_KEY").unwrap_err();
    assert!(
      matches!(err, AiError::Config(ref m) if m.contains("PHONON_TEST_DEFINITELY_UNSET_KEY"))
    );
  }

  /// Fails with `errors` in order, then succeeds; returns the call count.
  async fn run_with_errors(errors: Vec<AiError>) -> (Result<u32, AiError>, u32) {
    let calls = std::cell::Cell::new(0u32);
    let errors = std::cell::RefCell::new(errors.into_iter());
    let result = retry_with_base_delay(Duration::ZERO, || {
      calls.set(calls.get() + 1);
      let next = errors.borrow_mut().next();
      async move {
        match next {
          Some(e) => Err(e),
          None => Ok(7),
        }
      }
    })
    .await;
    (result, calls.get())
  }

  fn api(status: u16) -> AiError {
    AiError::Api {
      status,
      message: String::new(),
    }
  }

  #[tokio::test]
  async fn test_retry_recovers_from_transient_errors() {
    let (result, calls) = run_with_errors(vec![api(429), AiError::Http("reset".into())]).await;
    assert_eq!(result.unwrap(), 7);
    assert_eq!(calls, 3);
  }

  #[tokio::test]
  async fn test_retry_gives_up_after_max_attempts() {
    let (result, calls) = run_with_errors(vec![api(503), api(503), api(503), api(503)]).await;
    assert!(matches!(result, Err(AiError::Api { status: 503, .. })));
    assert_eq!(calls, MAX_ATTEMPTS);
  }

  #[tokio::test]
  async fn test_retry_does_not_repeat_permanent_errors() {
    let (result, calls) = run_with_errors(vec![api(401)]).await;
    assert!(matches!(result, Err(AiError::Api { status: 401, .. })));
    assert_eq!(calls, 1);
    let (result, calls) = run_with_errors(vec![AiError::Decode("bad mp3".into())]).await;
    assert!(result.is_err());
    assert_eq!(calls, 1);
  }
}
