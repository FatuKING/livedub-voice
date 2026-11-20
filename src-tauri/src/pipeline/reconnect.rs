//! Reconnecting a translation session that dropped mid-session.
//!
//! When a live session ends without being asked to (network loss, the server closing the
//! socket, a read or ping timeout, a session that expired), the pipeline keeps its devices open
//! and opens a new session with growing waits: [`ReconnectPolicy::delay`] doubles from
//! [`base_delay`](ReconnectPolicy::base_delay) up to [`max_delay`](ReconnectPolicy::max_delay),
//! never shorter than a retry hint from OpenAI (rate limits), for at most
//! [`max_attempts`](ReconnectPolicy::max_attempts) attempts. A fatal error (key rejected, no
//! quota, unsupported language) ends the direction at once.
//!
//! Attempts are counted across drops: the count only starts over once a reconnected session has
//! stayed up for [`stable_after`](ReconnectPolicy::stable_after). A server that accepts and then
//! drops every session therefore still runs out of attempts instead of reconnecting forever
//! (every session costs money and counts against rate limits).
//!
//! The waits use Tokio's clock, so tests run the schedule with a paused clock.

use std::sync::Arc;
use std::time::Duration;

use crate::settings::Language;
use crate::translator::{LiveSession, Translator, TranslatorError};

/// How a dropped session is reconnected. See the [module docs](self).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReconnectPolicy {
    /// Wait before the first attempt.
    pub base_delay: Duration,
    /// Longest wait between attempts (before retry hints).
    pub max_delay: Duration,
    /// Attempts before the direction stops with `reconnect_failed`.
    pub max_attempts: u32,
    /// How long a reconnected session must stay up for the attempt count to start over.
    pub stable_after: Duration,
    /// Longest wait a retry hint from OpenAI may impose.
    pub max_retry_hint: Duration,
}

impl Default for ReconnectPolicy {
    /// 1 s, 2 s, 4 s, 8 s, 16 s, 16 s: six attempts over ~47 s.
    fn default() -> Self {
        Self {
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(16),
            max_attempts: 6,
            stable_after: Duration::from_secs(30),
            max_retry_hint: Duration::from_secs(60),
        }
    }
}

impl ReconnectPolicy {
    /// Wait before attempt `attempt` (1-based): `base_delay * 2^(attempt - 1)`, capped at
    /// `max_delay`, and never shorter than `hint` (capped at `max_retry_hint`).
    pub fn delay(&self, attempt: u32, hint: Option<Duration>) -> Duration {
        let exponent = attempt.saturating_sub(1).min(30);
        let backoff = self
            .base_delay
            .saturating_mul(1 << exponent)
            .min(self.max_delay);
        match hint {
            Some(hint) => backoff.max(hint.min(self.max_retry_hint)),
            None => backoff,
        }
    }
}

/// Why reconnecting gave up.
#[derive(Debug)]
pub enum ReconnectError {
    /// An attempt failed with an error retrying cannot fix.
    Fatal(TranslatorError),
    /// Every attempt failed; `last` is the last attempt's error (or the drop's cause when no
    /// attempt was left).
    Exhausted {
        attempts: u32,
        last: TranslatorError,
    },
}

/// Opens a new session after `cause` dropped the previous one, trying attempts
/// `first_attempt..=policy.max_attempts`.
///
/// `on_attempt(n)` runs before each attempt's wait (to report "reconnecting n/max"). Returns the
/// session and the number of the attempt that opened it. Cancel it by dropping the future.
pub async fn reconnect(
    translator: Arc<dyn Translator>,
    target_language: Language,
    policy: ReconnectPolicy,
    first_attempt: u32,
    cause: TranslatorError,
    mut on_attempt: impl FnMut(u32) + Send,
) -> Result<(LiveSession, u32), ReconnectError> {
    if !cause.is_retryable() {
        return Err(ReconnectError::Fatal(cause));
    }
    let first_attempt = first_attempt.max(1);
    let mut last = cause;
    for attempt in first_attempt..=policy.max_attempts {
        on_attempt(attempt);
        let delay = policy.delay(attempt, last.retry_after());
        tracing::info!(
            provider = translator.name(),
            attempt,
            max_attempts = policy.max_attempts,
            delay_ms = delay.as_millis() as u64,
            cause = last.code(),
            "reconnecting the translation session"
        );
        tokio::time::sleep(delay).await;
        match translator.start(target_language).await {
            Ok(session) => {
                tracing::info!(attempt, "translation session reconnected");
                return Ok((session, attempt));
            }
            Err(error) if error.is_retryable() => {
                tracing::warn!(attempt, code = error.code(), %error, "reconnect attempt failed");
                last = error;
            }
            Err(error) => {
                tracing::warn!(attempt, code = error.code(), %error, "reconnect failed: fatal error");
                return Err(ReconnectError::Fatal(error));
            }
        }
    }
    Err(ReconnectError::Exhausted {
        attempts: policy.max_attempts,
        last,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use futures_util::future::BoxFuture;
    use tokio::time::Instant;

    use super::*;
    use crate::translator::FakeTranslator;

    #[test]
    fn delays_double_up_to_the_cap() {
        let policy = ReconnectPolicy::default();
        let delays: Vec<u64> = (1..=7).map(|n| policy.delay(n, None).as_secs()).collect();
        assert_eq!(delays, [1, 2, 4, 8, 16, 16, 16]);
        // A huge attempt number neither overflows nor passes the cap.
        assert_eq!(policy.delay(u32::MAX, None), policy.max_delay);
    }

    #[test]
    fn retry_hints_lengthen_the_wait_within_a_cap() {
        let policy = ReconnectPolicy::default();
        assert_eq!(
            policy.delay(1, Some(Duration::from_secs(5))),
            Duration::from_secs(5)
        );
        // A hint shorter than the backoff does not shorten it.
        assert_eq!(
            policy.delay(4, Some(Duration::from_millis(200))),
            Duration::from_secs(8)
        );
        assert_eq!(
            policy.delay(1, Some(Duration::from_secs(3600))),
            policy.max_retry_hint
        );
    }

    /// Fails `start` with the queued errors, then opens fake sessions. Records when each
    /// attempt happened.
    struct FlakyTranslator {
        failures: Mutex<VecDeque<TranslatorError>>,
        attempts: Mutex<Vec<Instant>>,
    }

    impl FlakyTranslator {
        fn new(failures: Vec<TranslatorError>) -> Arc<Self> {
            Arc::new(Self {
                failures: Mutex::new(failures.into()),
                attempts: Mutex::default(),
            })
        }
    }

    impl Translator for FlakyTranslator {
        fn name(&self) -> &'static str {
            "flaky"
        }

        fn start(&self, language: Language) -> BoxFuture<'_, Result<LiveSession, TranslatorError>> {
            Box::pin(async move {
                self.attempts.lock().unwrap().push(Instant::now());
                let failure = self.failures.lock().unwrap().pop_front();
                match failure {
                    Some(error) => Err(error),
                    None => {
                        FakeTranslator::echo(Duration::from_millis(10))
                            .start(language)
                            .await
                    }
                }
            })
        }
    }

    fn network() -> TranslatorError {
        TranslatorError::Network("connection reset".into())
    }

    fn offsets(started: Instant, translator: &FlakyTranslator) -> Vec<u64> {
        translator
            .attempts
            .lock()
            .unwrap()
            .iter()
            .map(|at| at.duration_since(started).as_millis() as u64)
            .collect()
    }

    #[tokio::test(start_paused = true)]
    async fn follows_the_backoff_schedule_until_a_session_opens() {
        let translator = FlakyTranslator::new(vec![network(), network(), network()]);
        let started = Instant::now();
        let mut reported = Vec::new();

        let (session, attempt) = reconnect(
            translator.clone(),
            Language::English,
            ReconnectPolicy::default(),
            1,
            network(),
            |n| reported.push(n),
        )
        .await
        .unwrap();

        assert_eq!(attempt, 4);
        assert_eq!(reported, [1, 2, 3, 4]);
        // Waits of 1, 2, 4 and 8 s before the attempts.
        assert_eq!(offsets(started, &translator), [1000, 3000, 7000, 15000]);
        drop(session);
    }

    #[tokio::test(start_paused = true)]
    async fn gives_up_after_the_last_attempt() {
        let translator = FlakyTranslator::new(vec![network(); 10]);
        let started = Instant::now();
        let policy = ReconnectPolicy {
            max_attempts: 3,
            ..ReconnectPolicy::default()
        };

        let error = reconnect(
            translator.clone(),
            Language::English,
            policy,
            1,
            network(),
            |_| {},
        )
        .await
        .err()
        .unwrap();

        assert!(
            matches!(&error, ReconnectError::Exhausted { attempts: 3, last } if last.code() == "network"),
            "{error:?}"
        );
        assert_eq!(offsets(started, &translator), [1000, 3000, 7000]);
    }

    #[tokio::test(start_paused = true)]
    async fn continues_the_count_of_an_earlier_drop() {
        let translator = FlakyTranslator::new(vec![network(); 10]);
        let policy = ReconnectPolicy {
            max_attempts: 3,
            ..ReconnectPolicy::default()
        };
        let mut reported = Vec::new();

        let error = reconnect(
            translator.clone(),
            Language::English,
            policy,
            3,
            network(),
            |n| reported.push(n),
        )
        .await
        .err()
        .unwrap();
        assert!(matches!(error, ReconnectError::Exhausted { .. }));
        assert_eq!(reported, [3]);

        // Nothing left: no attempt at all.
        let error = reconnect(
            translator.clone(),
            Language::English,
            policy,
            4,
            network(),
            |_| {},
        )
        .await
        .err()
        .unwrap();
        assert!(matches!(error, ReconnectError::Exhausted { .. }));
        assert_eq!(translator.attempts.lock().unwrap().len(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn stops_at_a_fatal_error() {
        let translator = FlakyTranslator::new(vec![network(), TranslatorError::QuotaExceeded]);

        let error = reconnect(
            translator.clone(),
            Language::English,
            ReconnectPolicy::default(),
            1,
            network(),
            |_| {},
        )
        .await
        .err()
        .unwrap();

        assert!(matches!(
            error,
            ReconnectError::Fatal(TranslatorError::QuotaExceeded)
        ));
        assert_eq!(translator.attempts.lock().unwrap().len(), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn a_fatal_cause_is_not_retried() {
        let translator = FlakyTranslator::new(Vec::new());

        let error = reconnect(
            translator.clone(),
            Language::English,
            ReconnectPolicy::default(),
            1,
            TranslatorError::Auth,
            |_| {},
        )
        .await
        .err()
        .unwrap();

        assert!(matches!(
            error,
            ReconnectError::Fatal(TranslatorError::Auth)
        ));
        assert!(translator.attempts.lock().unwrap().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn waits_at_least_as_long_as_the_rate_limit_hint() {
        let translator = FlakyTranslator::new(vec![TranslatorError::RateLimited {
            retry_after: Some(Duration::from_secs(20)),
        }]);
        let started = Instant::now();

        let (_session, attempt) = reconnect(
            translator.clone(),
            Language::English,
            ReconnectPolicy::default(),
            1,
            TranslatorError::RateLimited {
                retry_after: Some(Duration::from_secs(5)),
            },
            |_| {},
        )
        .await
        .unwrap();

        assert_eq!(attempt, 2);
        // 5 s (the drop's hint) instead of 1 s, then 20 s (the attempt's hint) instead of 2 s.
        assert_eq!(offsets(started, &translator), [5000, 25000]);
    }
}
