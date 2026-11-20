use std::sync::Mutex;
use std::time::Duration;

use chrono::TimeDelta;

use super::*;
use crate::pipeline::testing::{
    MemoryIo, TestTranslator, incoming_config, outgoing_config, recorder,
};
use crate::pipeline::{AudioIo, PipelineConfig, PipelineManager, PipelineStatus, StartPlan};
use crate::translator::Translator;

/// A clock the test moves by hand, in any time zone.
struct FakeClock(Mutex<DateTime<FixedOffset>>);

impl FakeClock {
    /// At `rfc3339`, e.g. `2026-11-20T10:00:00-03:00`.
    fn at(rfc3339: &str) -> Arc<Self> {
        Arc::new(Self(Mutex::new(
            DateTime::parse_from_rfc3339(rfc3339).unwrap(),
        )))
    }

    fn advance(&self, seconds: i64) {
        let mut now = self.0.lock().unwrap();
        *now += TimeDelta::seconds(seconds);
    }

    fn set(&self, rfc3339: &str) {
        *self.0.lock().unwrap() = DateTime::parse_from_rfc3339(rfc3339).unwrap();
    }
}

impl Clock for FakeClock {
    fn now(&self) -> DateTime<FixedOffset> {
        *self.0.lock().unwrap()
    }
}

fn date(text: &str) -> NaiveDate {
    text.parse().unwrap()
}

fn meter_at(rfc3339: &str, cap: SpendingCapSettings) -> (UsageMeter, Arc<FakeClock>) {
    let clock = FakeClock::at(rfc3339);
    let meter = UsageMeter::new(None, Arc::clone(&clock) as Arc<dyn Clock>, cap);
    (meter, clock)
}

fn no_cap() -> SpendingCapSettings {
    SpendingCapSettings::default()
}

fn daily(usd: f64) -> SpendingCapSettings {
    SpendingCapSettings {
        daily_usd: Some(usd),
        monthly_usd: None,
    }
}

fn close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-9,
        "expected {expected}, got {actual}"
    );
}

/// A sink that meters into `meter` and records the notices.
fn sink(
    meter: &Arc<UsageMeter>,
    direction: Direction,
) -> (EventSink, Arc<Mutex<Vec<UsageNotice>>>) {
    let notices = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&notices);
    let notify: UsageNotify = Arc::new(move |notice| recorded.lock().unwrap().push(notice));
    let events: EventSink = Arc::new(|_| {});
    (
        metered_sink(Arc::clone(meter), direction, events, notify),
        notices,
    )
}

fn status(state: PipelineState) -> PipelineEvent {
    PipelineEvent::Status(PipelineStatus {
        state,
        ..PipelineStatus::default()
    })
}

fn sent(seconds: f64) -> PipelineEvent {
    PipelineEvent::AudioSent { seconds }
}

#[test]
fn a_minute_costs_the_published_price() {
    close(cost_usd(60.0), 0.034);
    close(cost_usd(3600.0), 2.04);
    close(cost_usd(0.0), 0.0);
}

#[test]
fn the_panel_mirrors_the_price() {
    let pricing = include_str!("../../../src/lib/pricing.ts");
    assert!(
        pricing.contains(&format!("PRICE_PER_MINUTE_USD = {PRICE_PER_MINUTE_USD};")),
        "src/lib/pricing.ts must hold the same price as the core"
    );
}

#[test]
fn counts_a_session_from_start_to_stop() {
    let meter = Arc::new(meter_at("2026-11-20T10:00:00-03:00", no_cap()).0);
    let (events, notices) = sink(&meter, Direction::Outgoing);

    events(status(PipelineState::Connecting));
    events(status(PipelineState::Live));
    for seconds in [1.0, 2.0, 3.0, 4.5] {
        events(sent(seconds));
    }
    events(status(PipelineState::Stopping));
    // The trailing silence, then the final report.
    events(sent(6.0));
    events(status(PipelineState::Idle));

    let snapshot = meter.snapshot();
    assert!(!snapshot.session.outgoing.active);
    close(snapshot.session.outgoing.seconds, 6.0);
    close(snapshot.session.outgoing.cost_usd, cost_usd(6.0));
    close(snapshot.today.outgoing_seconds, 6.0);
    close(snapshot.today.incoming_seconds, 0.0);
    assert_eq!(snapshot.session.incoming, SessionUsage::default());
    {
        let notices = notices.lock().unwrap();
        assert_eq!(notices.first(), Some(&UsageNotice::Updated));
        assert_eq!(notices.last(), Some(&UsageNotice::SessionEnded));
    }

    // A second session starts from zero; the day keeps adding up.
    events(status(PipelineState::Connecting));
    assert!(meter.snapshot().session.outgoing.active);
    close(meter.snapshot().session.outgoing.seconds, 0.0);
    events(sent(2.0));
    events(status(PipelineState::Idle));
    let snapshot = meter.snapshot();
    close(snapshot.session.outgoing.seconds, 2.0);
    close(snapshot.today.outgoing_seconds, 8.0);
    assert_eq!(snapshot.days.len(), 1);
    assert_eq!(
        meter.lock().file.days[&date("2026-11-20")]
            .outgoing
            .sessions,
        2
    );
}

#[test]
fn reconnect_gaps_are_not_counted() {
    let (meter, clock) = meter_at("2026-11-20T10:00:00-03:00", no_cap());
    let meter = Arc::new(meter);
    let (events, _) = sink(&meter, Direction::Incoming);

    events(status(PipelineState::Connecting));
    events(sent(10.0));
    // The session drops: no audio is appended for 8 s of wall-clock time.
    events(status(PipelineState::Reconnecting));
    for _ in 0..8 {
        clock.advance(1);
        events(sent(10.0));
    }
    events(status(PipelineState::Live));
    clock.advance(5);
    events(sent(15.0));
    events(status(PipelineState::Idle));

    close(meter.snapshot().session.incoming.seconds, 15.0);
    close(meter.snapshot().today.incoming_seconds, 15.0);
}

#[test]
fn reports_that_go_backwards_add_nothing() {
    let meter = meter_at("2026-11-20T10:00:00Z", no_cap()).0;
    meter.begin_session(Direction::Outgoing);
    meter.record_sent(Direction::Outgoing, 5.0);
    meter.record_sent(Direction::Outgoing, 3.0);
    meter.record_sent(Direction::Outgoing, 5.5);

    close(meter.snapshot().today.outgoing_seconds, 5.5);
}

#[test]
fn both_directions_at_once_add_up() {
    let meter = Arc::new(meter_at("2026-11-20T10:00:00-03:00", no_cap()).0);
    let (incoming, _) = sink(&meter, Direction::Incoming);
    let (outgoing, _) = sink(&meter, Direction::Outgoing);

    incoming(status(PipelineState::Connecting));
    outgoing(status(PipelineState::Connecting));
    for second in 1..=30 {
        incoming(sent(f64::from(second)));
        outgoing(sent(f64::from(second) * 0.5));
    }
    outgoing(status(PipelineState::Idle));

    let snapshot = meter.snapshot();
    assert!(snapshot.session.incoming.active);
    assert!(!snapshot.session.outgoing.active);
    close(snapshot.session.incoming.seconds, 30.0);
    close(snapshot.session.outgoing.seconds, 15.0);
    close(snapshot.today.incoming_seconds, 30.0);
    close(snapshot.today.outgoing_seconds, 15.0);
    close(snapshot.today.cost_usd, cost_usd(45.0));
    close(snapshot.month.cost_usd, cost_usd(45.0));
    assert_eq!(snapshot.days.len(), 1);
    close(snapshot.days[0].cost_usd, cost_usd(45.0));
}

#[test]
fn a_session_across_midnight_splits_between_the_days() {
    let (meter, clock) = meter_at("2026-11-20T23:59:50-03:00", no_cap());
    meter.begin_session(Direction::Outgoing);
    meter.record_sent(Direction::Outgoing, 5.0);
    clock.advance(20); // 00:00:10 on the 21st, local time.
    meter.record_sent(Direction::Outgoing, 25.0);

    let snapshot = meter.snapshot();
    close(snapshot.today.outgoing_seconds, 20.0);
    close(snapshot.month.outgoing_seconds, 25.0);
    close(snapshot.session.outgoing.seconds, 25.0);
    let dates: Vec<_> = snapshot.days.iter().map(|day| day.date).collect();
    assert_eq!(dates, [date("2026-11-21"), date("2026-11-20")]);
    // It counts as a session on both days.
    let state = meter.lock();
    assert_eq!(state.file.days[&date("2026-11-20")].outgoing.sessions, 1);
    assert_eq!(state.file.days[&date("2026-11-21")].outgoing.sessions, 1);
}

#[test]
fn the_month_rolls_over_at_local_midnight() {
    let (meter, clock) = meter_at("2026-11-30T23:00:00+09:00", no_cap());
    meter.begin_session(Direction::Incoming);
    meter.record_sent(Direction::Incoming, 120.0);
    close(meter.snapshot().month.incoming_seconds, 120.0);

    clock.set("2026-12-01T00:00:01+09:00");
    let snapshot = meter.snapshot();
    close(snapshot.today.incoming_seconds, 0.0);
    close(snapshot.month.incoming_seconds, 0.0);
    // The history keeps November.
    assert_eq!(snapshot.days.len(), 1);
    assert_eq!(snapshot.days[0].date, date("2026-11-30"));

    meter.record_sent(Direction::Incoming, 180.0);
    let snapshot = meter.snapshot();
    close(snapshot.month.incoming_seconds, 60.0);
    close(snapshot.today.incoming_seconds, 60.0);
}

#[test]
fn days_follow_the_local_time_zone() {
    // The same instant (02:00 UTC on the 20th) is still the 19th in Buenos Aires and already
    // the 20th in Tokyo.
    for (rfc3339, expected) in [
        ("2026-11-19T23:00:00-03:00", "2026-11-19"),
        ("2026-11-20T11:00:00+09:00", "2026-11-20"),
    ] {
        let meter = meter_at(rfc3339, no_cap()).0;
        meter.begin_session(Direction::Outgoing);
        meter.record_sent(Direction::Outgoing, 10.0);
        assert_eq!(meter.snapshot().days[0].date, date(expected), "{rfc3339}");
    }
}

#[test]
fn a_time_zone_change_moves_later_audio_to_the_new_local_day() {
    let (meter, clock) = meter_at("2026-11-20T22:00:00-03:00", no_cap());
    meter.begin_session(Direction::Outgoing);
    meter.record_sent(Direction::Outgoing, 10.0);
    // Same instant, but the PC now uses UTC+1 (it is 02:00 on the 21st there).
    clock.set("2026-11-21T02:00:00+01:00");
    meter.record_sent(Direction::Outgoing, 15.0);

    let snapshot = meter.snapshot();
    close(snapshot.today.outgoing_seconds, 5.0);
    assert_eq!(snapshot.days[1].date, date("2026-11-20"));
}

#[test]
fn history_is_newest_first_and_bounded() {
    let (meter, clock) = meter_at("2026-01-01T12:00:00Z", no_cap());
    for _ in 0..(HISTORY_DAYS + 10) {
        meter.begin_session(Direction::Incoming);
        meter.record_sent(Direction::Incoming, 1.0);
        clock.advance(24 * 3600);
    }
    let days = meter.snapshot().days;
    assert_eq!(days.len(), HISTORY_DAYS);
    assert!(days.windows(2).all(|pair| pair[0].date > pair[1].date));
}

#[test]
fn persists_atomically_and_reloads() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(USAGE_FILE_NAME);
    let clock = FakeClock::at("2026-11-20T10:00:00-03:00");
    let meter = UsageMeter::new(Some(path.clone()), clock.clone(), no_cap());

    // Nothing to write yet.
    meter.flush().unwrap();
    assert!(!path.exists());

    meter.begin_session(Direction::Incoming);
    meter.record_sent(Direction::Incoming, 42.0);
    meter.override_today();
    meter.flush().unwrap();
    assert!(path.exists());
    assert!(!crate::persist::with_suffix(&path, ".tmp").exists());
    let written: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(written["version"], 1);
    assert_eq!(written["days"]["2026-11-20"]["incoming"]["seconds"], 42.0);
    assert_eq!(written["capOverrideDate"], "2026-11-20");

    let reloaded = UsageMeter::new(Some(path), clock, no_cap());
    let snapshot = reloaded.snapshot();
    close(snapshot.today.incoming_seconds, 42.0);
    // A new run has no running session.
    assert_eq!(snapshot.session.incoming, SessionUsage::default());
    assert!(snapshot.cap.overridden_today);
}

#[test]
fn flush_writes_only_changes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(USAGE_FILE_NAME);
    let meter = UsageMeter::new(
        Some(path.clone()),
        FakeClock::at("2026-11-20T10:00:00Z"),
        no_cap(),
    );
    meter.begin_session(Direction::Outgoing);
    meter.record_sent(Direction::Outgoing, 1.0);
    meter.flush().unwrap();
    std::fs::write(&path, "sentinel").unwrap();

    meter.flush().unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "sentinel");

    meter.record_sent(Direction::Outgoing, 2.0);
    meter.flush().unwrap();
    assert_ne!(std::fs::read_to_string(&path).unwrap(), "sentinel");
}

#[test]
fn a_cap_warns_at_80_percent_and_trips_once_at_100() {
    // $0.034 a day = one minute of audio.
    let meter = meter_at("2026-11-20T10:00:00-03:00", daily(0.034)).0;
    meter.begin_session(Direction::Outgoing);

    let check = meter.record_sent(Direction::Outgoing, 47.0);
    assert_eq!(check.status.level, CapLevel::Ok);
    assert_eq!(check.status.period, Some(CapPeriod::Daily));
    assert!(!check.trip);

    let check = meter.record_sent(Direction::Outgoing, 48.0);
    assert_eq!(check.status.level, CapLevel::Warning);
    assert!(!check.trip);
    assert!(meter.check_start().is_ok());

    let check = meter.record_sent(Direction::Outgoing, 60.0);
    assert_eq!(check.status.level, CapLevel::Reached);
    close(check.status.spent_usd, 0.034);
    assert_eq!(check.status.limit_usd, Some(0.034));
    assert!(check.trip);

    // Still reached while the stop finishes: no second trip.
    let check = meter.record_sent(Direction::Outgoing, 61.5);
    assert_eq!(check.status.level, CapLevel::Reached);
    assert!(!check.trip);

    let error = meter.check_start().unwrap_err();
    assert_eq!(error.code(), "spending_cap_reached");
}

#[test]
fn no_cap_never_trips() {
    let meter = meter_at("2026-11-20T10:00:00Z", no_cap()).0;
    meter.begin_session(Direction::Incoming);
    let check = meter.record_sent(Direction::Incoming, 100_000.0);
    assert_eq!(check.status.level, CapLevel::Off);
    assert_eq!(check.status.period, None);
    assert!(!check.trip);
    assert!(meter.check_start().is_ok());
}

#[test]
fn the_monthly_cap_counts_the_whole_month() {
    let caps = SpendingCapSettings {
        daily_usd: Some(1.0),
        monthly_usd: Some(0.068), // two minutes
    };
    let (meter, clock) = meter_at("2026-11-01T10:00:00-03:00", caps);
    meter.begin_session(Direction::Incoming);
    meter.record_sent(Direction::Incoming, 60.0);
    clock.set("2026-11-15T10:00:00-03:00");
    meter.begin_session(Direction::Incoming);
    let check = meter.record_sent(Direction::Incoming, 50.0);
    // 110 s of 120 s this month; the daily cap is far away.
    assert_eq!(check.status.period, Some(CapPeriod::Monthly));
    assert_eq!(check.status.level, CapLevel::Warning);

    let check = meter.record_sent(Direction::Incoming, 60.0);
    assert_eq!(check.status.level, CapLevel::Reached);
    assert!(check.trip);

    // A new month starts over.
    clock.set("2026-12-01T00:00:00-03:00");
    assert_eq!(meter.cap_status().level, CapLevel::Ok);
    assert!(meter.check_start().is_ok());
}

#[test]
fn override_lasts_until_the_end_of_the_local_day() {
    let (meter, clock) = meter_at("2026-11-20T10:00:00-03:00", daily(0.034));
    meter.begin_session(Direction::Outgoing);
    assert!(meter.record_sent(Direction::Outgoing, 60.0).trip);

    meter.override_today();
    assert!(meter.check_start().is_ok());
    let check = meter.record_sent(Direction::Outgoing, 120.0);
    assert_eq!(check.status.level, CapLevel::Reached);
    assert!(check.status.overridden_today);
    assert!(!check.status.enforced());
    assert!(!check.trip);

    // Tomorrow the daily spend starts over and the override no longer applies.
    clock.set("2026-11-21T00:00:01-03:00");
    let status = meter.cap_status();
    assert!(!status.overridden_today);
    assert_eq!(status.level, CapLevel::Ok);
    meter.begin_session(Direction::Outgoing);
    let check = meter.record_sent(Direction::Outgoing, 60.0);
    assert!(check.trip, "the cap applies again on a new day");
}

#[test]
fn raising_the_cap_lets_it_trip_again_later() {
    let meter = meter_at("2026-11-20T10:00:00Z", daily(0.034)).0;
    meter.begin_session(Direction::Outgoing);
    assert!(meter.record_sent(Direction::Outgoing, 60.0).trip);

    meter.set_cap(daily(0.068));
    assert!(meter.check_start().is_ok());
    let check = meter.record_sent(Direction::Outgoing, 100.0);
    assert_eq!(check.status.level, CapLevel::Warning);
    assert!(!check.trip);
    assert!(meter.record_sent(Direction::Outgoing, 120.0).trip);
}

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

fn last_audio_sent(events: &[PipelineEvent]) -> f64 {
    events
        .iter()
        .rev()
        .find_map(|event| match event {
            PipelineEvent::AudioSent { seconds } => Some(*seconds),
            _ => None,
        })
        .unwrap_or_default()
}

#[tokio::test]
async fn meters_what_a_real_pipeline_sends() {
    let meter = Arc::new(meter_at("2026-11-20T10:00:00-03:00", no_cap()).0);
    let manager = PipelineManager::new();
    let io = Arc::new(MemoryIo::new());
    let translator = TestTranslator::echo();
    let (inner, recorded) = recorder();
    let notify: UsageNotify = Arc::new(|_| {});
    let events = metered_sink(Arc::clone(&meter), Direction::Outgoing, inner, notify);

    manager
        .start(
            Direction::Outgoing,
            events,
            plan(outgoing_config(), &translator, &io),
        )
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(1300)).await;
    assert!(meter.snapshot().session.outgoing.active);
    manager.stop(Direction::Outgoing).await;

    let sent = last_audio_sent(&recorded.lock().unwrap());
    assert!(sent > 1.0, "{sent}");
    let snapshot = meter.snapshot();
    assert!(!snapshot.session.outgoing.active);
    close(snapshot.session.outgoing.seconds, sent);
    close(snapshot.today.outgoing_seconds, sent);
}

#[tokio::test]
async fn reaching_the_cap_stops_both_directions_through_the_manager() {
    // $0.0002 ≈ 0.35 s of audio: reached at the first report of either direction.
    let meter = Arc::new(meter_at("2026-11-20T10:00:00-03:00", daily(0.0002)).0);
    let manager = Arc::new(PipelineManager::new());
    let io = Arc::new(MemoryIo::new());
    let translator = TestTranslator::echo();
    let (inner, recorded) = recorder();
    let trips = Arc::new(Mutex::new(Vec::new()));
    let notify: UsageNotify = {
        let manager = Arc::clone(&manager);
        let trips = Arc::clone(&trips);
        let runtime = tokio::runtime::Handle::current();
        Arc::new(move |notice| {
            if let UsageNotice::CapReached(status) = notice {
                trips.lock().unwrap().push(status);
                let manager = Arc::clone(&manager);
                // Called from the pump thread: hand the stop to the runtime.
                runtime.spawn(async move { manager.stop_all().await });
            }
        })
    };
    for (direction, config) in [
        (Direction::Incoming, incoming_config()),
        (Direction::Outgoing, outgoing_config()),
    ] {
        let events = metered_sink(
            Arc::clone(&meter),
            direction,
            Arc::clone(&inner),
            Arc::clone(&notify),
        );
        manager
            .start(direction, events, plan(config, &translator, &io))
            .await
            .unwrap();
    }
    assert!((io.app_volume() - 0.2).abs() < 1e-6, "ducked while live");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    while manager.status(Direction::Incoming) != PipelineStatus::idle()
        || manager.status(Direction::Outgoing) != PipelineStatus::idle()
        || translator.live_sessions() > 0
    {
        assert!(
            tokio::time::Instant::now() < deadline,
            "the cap did not stop both directions"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    // Tripped once, stopped gracefully (through `stopping`), original audio restored.
    let trips = trips.lock().unwrap();
    assert_eq!(trips.len(), 1);
    assert_eq!(trips[0].level, CapLevel::Reached);
    let events = recorded.lock().unwrap();
    assert!(
        crate::pipeline::testing::states(&events).contains(&PipelineState::Stopping),
        "a graceful stop"
    );
    assert_eq!(io.app_volume(), 1.0);
    assert_eq!(io.streams_alive(), 0);
    let snapshot = meter.snapshot();
    assert!(!snapshot.session.incoming.active && !snapshot.session.outgoing.active);
    assert!(snapshot.session.incoming.seconds > 0.0);
    assert_eq!(
        meter.check_start().unwrap_err().code(),
        "spending_cap_reached"
    );
    // Once the user keeps going for today, starting works again.
    meter.override_today();
    assert!(meter.check_start().is_ok());
}
