//! The one owner of both translation directions.
//!
//! [`PipelineManager`] holds at most one [`TranslationPipeline`] per [`Direction`] and the latest
//! status of each:
//! - **Independent:** each direction starts and stops on its own.
//! - **Idempotent:** a start while the direction runs (or is starting) does nothing, and a stop
//!   while it is stopped does nothing. Each direction's slot is locked for the whole start or
//!   stop, so a double click waits for the first call and then finds the work done: it never
//!   opens a second session or the devices twice.
//! - **Settings apply on the next start.** A running direction keeps the devices, language and
//!   source it started with (only the original-audio volume changes live). Restarting on every
//!   save would cut the sentence being translated, open a new paid session and reopen the
//!   devices while the user may still be editing; the panel says so next to the settings.
//! - **Clean exit:** [`shutdown`](PipelineManager::shutdown) stops both directions at once (no
//!   tail: `session.close`, WASAPI threads joined, cable released, ducked apps restored) within a
//!   bounded time, turning a graceful stop already in progress into a fast one, and refuses new
//!   starts.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use super::translation::{Direction, EventSink, PipelineConfig, PipelineEvent, StopHandle};
use super::{AudioIo, PipelineStatus, TranslationPipeline};
use crate::error::{AppError, AppResult};
use crate::translator::Translator;

/// What a direction needs to start, from the settings and the credentials.
pub struct StartPlan {
    pub config: PipelineConfig,
    pub translator: Arc<dyn Translator>,
    pub io: Arc<dyn AudioIo>,
}

/// One direction: its pipeline (at most one) and latest status.
#[derive(Default)]
struct Slot {
    /// Locked for a whole start or stop.
    pipeline: tokio::sync::Mutex<Option<TranslationPipeline>>,
    /// The running pipeline's stop handle, reachable while a stop holds the lock.
    stop: Mutex<Option<StopHandle>>,
    status: Arc<Mutex<PipelineStatus>>,
}

/// Owns both directions. See the [module docs](self).
#[derive(Default)]
pub struct PipelineManager {
    incoming: Slot,
    outgoing: Slot,
    /// Set by [`shutdown`](Self::shutdown): no more starts.
    closing: AtomicBool,
}

impl PipelineManager {
    pub fn new() -> Self {
        Self::default()
    }

    fn slot(&self, direction: Direction) -> &Slot {
        match direction {
            Direction::Incoming => &self.incoming,
            Direction::Outgoing => &self.outgoing,
        }
    }

    /// Starts `direction` unless it already runs. `prepare` reads the settings and checks the
    /// devices (it only runs when a start is needed); its failure is reported like a start
    /// failure. `events` receives the pipeline's events.
    pub async fn start(
        &self,
        direction: Direction,
        events: EventSink,
        prepare: impl Future<Output = AppResult<StartPlan>>,
    ) -> AppResult<()> {
        let slot = self.slot(direction);
        let events = recording_sink(Arc::clone(&slot.status), events);
        let mut pipeline = slot.pipeline.lock().await;
        if pipeline
            .as_ref()
            .is_some_and(|pipeline| !pipeline.is_finished())
        {
            tracing::debug!(direction = direction.name(), "already running");
            return Ok(());
        }
        if let Some(finished) = pipeline.take() {
            // Stopped on its own (fatal error): only its task is left to reap.
            finished.stop().await;
        }
        if self.closing.load(Ordering::SeqCst) {
            return Err(closing());
        }
        let plan = match prepare.await {
            Ok(plan) => plan,
            Err(error) => {
                tracing::warn!(direction = direction.name(), code = error.code(), %error, "translation refused to start");
                events(PipelineEvent::Status(PipelineStatus::error(&error)));
                return Err(error);
            }
        };
        // The pipeline reports its own start failures.
        let started =
            TranslationPipeline::start(plan.config, plan.translator, plan.io, events).await?;
        *lock(&slot.stop) = Some(started.stop_handle());
        if self.closing.load(Ordering::SeqCst) {
            // The app began closing while this direction was starting.
            started.stop_handle().stop_now();
        }
        *pipeline = Some(started);
        Ok(())
    }

    /// Stops `direction` (gracefully: the tail of the last sentence plays) and waits until it
    /// has stopped. Does nothing if it is not running.
    pub async fn stop(&self, direction: Direction) {
        let slot = self.slot(direction);
        // Locked until stopped (up to ~10 s), so a start cannot reopen the devices before they
        // are released.
        let mut pipeline = slot.pipeline.lock().await;
        if let Some(running) = pipeline.take() {
            running.stop().await;
        }
        *lock(&slot.stop) = None;
    }

    /// `direction`'s latest status.
    pub fn status(&self, direction: Direction) -> PipelineStatus {
        lock(&self.slot(direction).status).clone()
    }

    /// Applies a new original-audio volume to a running incoming translation. Never waits: if
    /// it is starting or stopping, the volume applies on its next start.
    pub fn set_original_volume(&self, percent: u8) {
        if let Ok(pipeline) = self.incoming.pipeline.try_lock()
            && let Some(pipeline) = pipeline.as_ref()
        {
            pipeline.set_original_volume(percent);
        }
    }

    /// Restores the apps a running incoming translation ducked, without waiting for it (last
    /// resort when [`shutdown`](Self::shutdown) ran out of time). Blocking.
    pub fn release_ducking(&self) {
        if let Ok(pipeline) = self.incoming.pipeline.try_lock()
            && let Some(pipeline) = pipeline.as_ref()
        {
            pipeline.release_ducking();
        }
    }

    /// Stops both directions at once (see the module docs) and refuses later starts. Returns
    /// whether everything was released within `limit`.
    pub async fn shutdown(&self, limit: Duration) -> bool {
        self.closing.store(true, Ordering::SeqCst);
        for slot in [&self.incoming, &self.outgoing] {
            // A graceful stop holding the lock finishes at once.
            if let Some(stop) = lock(&slot.stop).as_ref() {
                stop.stop_now();
            }
        }
        let both =
            async { tokio::join!(shutdown_slot(&self.incoming), shutdown_slot(&self.outgoing)) };
        match tokio::time::timeout(limit, both).await {
            Ok(_) => {
                tracing::info!("translations stopped for exit");
                true
            }
            Err(_) => {
                tracing::warn!(
                    limit_ms = limit.as_millis() as u64,
                    "translations did not stop in time for exit"
                );
                false
            }
        }
    }
}

/// Stops a slot's pipeline at once (waiting for a start or stop in progress first).
async fn shutdown_slot(slot: &Slot) {
    let mut pipeline = slot.pipeline.lock().await;
    if let Some(running) = pipeline.take() {
        running.shutdown().await;
    }
    *lock(&slot.stop) = None;
}

fn closing() -> AppError {
    AppError::Internal("LiveDub Voice is closing".into())
}

/// Remembers the latest status in `status` before forwarding each event to `events`.
fn recording_sink(status: Arc<Mutex<PipelineStatus>>, events: EventSink) -> EventSink {
    Arc::new(move |event| {
        if let PipelineEvent::Status(new) = &event {
            *lock(&status) = new.clone();
        }
        events(event);
    })
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;
    use std::time::Instant;

    use super::*;
    use crate::pipeline::PipelineState;
    use crate::pipeline::testing::{
        MemoryIo, TestTranslator, incoming_config, outgoing_config, recorder, states,
    };

    fn plan(
        config: PipelineConfig,
        translator: &Arc<TestTranslator>,
        io: &Arc<MemoryIo>,
    ) -> impl Future<Output = AppResult<StartPlan>> + use<> {
        let plan = StartPlan {
            config,
            translator: Arc::clone(translator) as Arc<dyn Translator>,
            io: Arc::clone(io) as Arc<dyn AudioIo>,
        };
        async move { Ok(plan) }
    }

    /// Everything a pipeline held is gone: streams (their threads), sessions (their tasks), the
    /// pump thread and the supervisor (both hold the event sink).
    fn assert_nothing_left(io: &MemoryIo, translator: &TestTranslator, events: &EventSink) {
        assert_eq!(io.streams_alive(), 0, "audio streams left running");
        assert_eq!(
            translator.live_sessions(),
            0,
            "translation sessions left open"
        );
        assert_eq!(
            Arc::strong_count(events),
            1,
            "pipeline threads or tasks still hold the event sink"
        );
    }

    #[tokio::test]
    async fn toggling_many_times_leaves_nothing_behind() {
        let manager = PipelineManager::new();
        let io = Arc::new(MemoryIo::new());
        let translator = TestTranslator::echo();
        let (events, recorded) = recorder();
        let cycles = 4;

        for _ in 0..cycles {
            // A double click on Start, then on Stop.
            let (first, second) = tokio::join!(
                manager.start(
                    Direction::Outgoing,
                    Arc::clone(&events),
                    plan(outgoing_config(), &translator, &io)
                ),
                manager.start(
                    Direction::Outgoing,
                    Arc::clone(&events),
                    plan(outgoing_config(), &translator, &io)
                ),
            );
            first.unwrap();
            second.unwrap();
            assert_eq!(
                manager.status(Direction::Outgoing).state,
                PipelineState::Live
            );
            tokio::time::sleep(Duration::from_millis(200)).await;
            tokio::join!(
                manager.stop(Direction::Outgoing),
                manager.stop(Direction::Outgoing)
            );
            assert_eq!(manager.status(Direction::Outgoing), PipelineStatus::idle());
            assert_nothing_left(&io, &translator, &events);
        }

        // One session and one pair of devices per cycle, never two.
        assert_eq!(translator.starts(), cycles);
        assert_eq!(io.streams_opened(), 2 * cycles);
        let states = states(&recorded.lock().unwrap());
        let cycle = [
            PipelineState::Connecting,
            PipelineState::Live,
            PipelineState::Stopping,
            PipelineState::Idle,
        ];
        assert_eq!(states, cycle.repeat(cycles));
        // The incoming direction was never touched.
        assert_eq!(manager.status(Direction::Incoming), PipelineStatus::idle());
    }

    #[tokio::test]
    async fn directions_are_independent() {
        let manager = PipelineManager::new();
        let io = Arc::new(MemoryIo::new());
        let translator = TestTranslator::echo();
        let (events, _recorded) = recorder();

        manager
            .start(
                Direction::Incoming,
                Arc::clone(&events),
                plan(incoming_config(), &translator, &io),
            )
            .await
            .unwrap();
        manager
            .start(
                Direction::Outgoing,
                Arc::clone(&events),
                plan(outgoing_config(), &translator, &io),
            )
            .await
            .unwrap();
        manager.stop(Direction::Outgoing).await;

        assert_eq!(
            manager.status(Direction::Incoming).state,
            PipelineState::Live
        );
        assert_eq!(manager.status(Direction::Outgoing), PipelineStatus::idle());
        assert_eq!(translator.live_sessions(), 1);
        // Stopping a stopped direction does nothing.
        manager.stop(Direction::Outgoing).await;
        manager.stop(Direction::Incoming).await;
        assert_nothing_left(&io, &translator, &events);
    }

    #[tokio::test]
    async fn a_failed_preparation_reports_the_error() {
        let manager = PipelineManager::new();
        let (events, recorded) = recorder();

        let error = manager
            .start(Direction::Outgoing, events, async {
                Err(AppError::NoOutputDevice)
            })
            .await
            .unwrap_err();

        assert_eq!(error.code(), "no_output_device");
        let status = manager.status(Direction::Outgoing);
        assert_eq!(status.state, PipelineState::Error);
        assert_eq!(status.error.unwrap().code, "no_output_device");
        assert_eq!(states(&recorded.lock().unwrap()), [PipelineState::Error]);
    }

    #[tokio::test]
    async fn shutdown_releases_both_directions_quickly_and_refuses_new_starts() {
        let manager = PipelineManager::new();
        let io = Arc::new(MemoryIo::new());
        let translator = TestTranslator::echo();
        let (events, recorded) = recorder();
        for (direction, config) in [
            (Direction::Incoming, incoming_config()),
            (Direction::Outgoing, outgoing_config()),
        ] {
            manager
                .start(
                    direction,
                    Arc::clone(&events),
                    plan(config, &translator, &io),
                )
                .await
                .unwrap();
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!((io.app_volume() - 0.2).abs() < 1e-6, "ducked while live");

        let started = Instant::now();
        assert!(manager.shutdown(Duration::from_secs(5)).await);
        let took = started.elapsed();

        // No trailing silence, no tail: well under a graceful stop (1.5 s of silence alone).
        assert!(took < Duration::from_millis(1200), "{took:?}");
        assert_nothing_left(&io, &translator, &events);
        assert_eq!(io.app_volume(), 1.0, "the original audio is restored");
        assert_eq!(manager.status(Direction::Incoming), PipelineStatus::idle());
        assert_eq!(manager.status(Direction::Outgoing), PipelineStatus::idle());
        assert!(
            !states(&recorded.lock().unwrap()).contains(&PipelineState::Stopping),
            "a fast stop does not finish the sentence"
        );

        let error = manager
            .start(
                Direction::Outgoing,
                Arc::clone(&events),
                plan(outgoing_config(), &translator, &io),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code(), "internal");
        assert_eq!(translator.starts(), 2);
    }

    #[tokio::test]
    async fn shutdown_cuts_a_graceful_stop_short() {
        let manager = Arc::new(PipelineManager::new());
        let io = Arc::new(MemoryIo::new());
        let translator = TestTranslator::echo();
        let (events, _recorded) = recorder();
        manager
            .start(
                Direction::Outgoing,
                Arc::clone(&events),
                plan(outgoing_config(), &translator, &io),
            )
            .await
            .unwrap();

        let stopping = {
            let manager = Arc::clone(&manager);
            tokio::spawn(async move { manager.stop(Direction::Outgoing).await })
        };
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(
            manager.status(Direction::Outgoing).state,
            PipelineState::Stopping
        );
        let started = Instant::now();
        assert!(manager.shutdown(Duration::from_secs(5)).await);
        stopping.await.unwrap();

        // The 1.5 s of trailing silence was not waited for.
        assert!(started.elapsed() < Duration::from_millis(1200));
        assert_nothing_left(&io, &translator, &events);
        assert!(io.input_stopped.load(Ordering::Relaxed));
    }
}
