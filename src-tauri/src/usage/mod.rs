//! Local usage metering and the in-app spending cap.
//!
//! OpenAI's Usage/Costs API needs an Admin key, so LiveDub Voice counts usage itself.
//!
//! # What counts as a billed minute
//!
//! `gpt-realtime-translate` costs $0.034 per minute (see [`PRICE_PER_MINUTE_USD`]); OpenAI
//! documents no rounding and does not say what the minute measures. Model time follows the audio
//! appended (a pause in appending does not exist for the model), so the app takes the billed
//! minute to be audio appended. The meter counts exactly that: the seconds of PCM appended to
//! the session, as the pipeline reports them (`PipelineEvent::AudioSent`, every second and once
//! more when it stops). With voice detection (see [`crate::pipeline::voice`]) that is speech,
//! its pre-roll and the ~1.2 s after each phrase; silence that is not appended is not counted,
//! and neither are reconnect gaps. With pausing off it equals the time the session is open and
//! streaming. It is an estimate: the real bill is on platform.openai.com.
//!
//! # History
//!
//! Seconds are added to the local calendar day (the user's time zone) of the moment they are
//! reported, per direction (see [`file`]). The history is kept in memory and written atomically
//! to `usage.json` by [`UsageMeter::flush`]: periodically ([`FLUSH_INTERVAL`]) while
//! translating, when a session ends and on exit, so a crash loses at most that interval.
//!
//! # Spending cap
//!
//! An optional daily and/or monthly cap in USD (`spendingCap` in the settings). From
//! [`WARNING_FRACTION`] (80 %) of a cap the panel shows a warning; at 100 % the running
//! directions are stopped (gracefully, through the pipeline manager) and starting a direction is
//! refused with `spending_cap_reached` until the user raises the cap, the day (or month) rolls
//! over, or the user chooses to keep going for the rest of today
//! ([`UsageMeter::override_today`]).

pub mod file;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use chrono::{DateTime, Datelike, FixedOffset, Local, NaiveDate};
use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::pipeline::{Direction, EventSink, PipelineEvent, PipelineState};
use crate::settings::SpendingCapSettings;
use file::{DayRecord, DirectionDay, UsageFile};

pub use file::USAGE_FILE_NAME;

/// OpenAI price of `gpt-realtime-translate`, in US dollars per audio minute. The one place the
/// price lives in the core (`src/lib/pricing.ts` mirrors it for the panel's static texts).
pub const PRICE_PER_MINUTE_USD: f64 = 0.034;

/// How often the usage history is written to disk while it changes.
pub const FLUSH_INTERVAL: Duration = Duration::from_secs(30);

/// Fraction of a spending cap from which the panel warns.
pub const WARNING_FRACTION: f64 = 0.8;

/// Most days the panel's history list gets (newest first).
pub const HISTORY_DAYS: usize = 90;

/// Estimated cost of `seconds` of audio, in US dollars.
pub fn cost_usd(seconds: f64) -> f64 {
    seconds / 60.0 * PRICE_PER_MINUTE_USD
}

/// The current local time. Mockable, so day and month rollovers can be tested.
pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<FixedOffset>;
}

/// The system clock in the system time zone.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<FixedOffset> {
        Local::now().fixed_offset()
    }
}

/// Which spending cap a [`CapStatus`] is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CapPeriod {
    Daily,
    Monthly,
}

impl std::fmt::Display for CapPeriod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Daily => "daily",
            Self::Monthly => "monthly",
        })
    }
}

/// How close the spend is to the spending cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CapLevel {
    /// No cap is set.
    Off,
    /// Under [`WARNING_FRACTION`] of every cap.
    Ok,
    /// At or over [`WARNING_FRACTION`] of a cap.
    Warning,
    /// At or over a cap.
    Reached,
}

/// The spending cap as the panel sees it: the cap closest to being reached (the highest
/// fraction spent; the daily one on a tie).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapStatus {
    pub daily_usd: Option<f64>,
    pub monthly_usd: Option<f64>,
    pub level: CapLevel,
    /// The cap `level` is about; `None` when no cap is set.
    pub period: Option<CapPeriod>,
    /// Estimated spend of that period (today or this month).
    pub spent_usd: f64,
    /// That cap.
    pub limit_usd: Option<f64>,
    /// The user chose to keep translating over the cap for the rest of today.
    pub overridden_today: bool,
}

impl CapStatus {
    /// Whether the cap stops translations now (reached and not overridden today).
    pub fn enforced(&self) -> bool {
        self.level == CapLevel::Reached && !self.overridden_today
    }

    fn error(&self) -> AppError {
        AppError::SpendingCapReached {
            period: self.period.unwrap_or(CapPeriod::Daily),
            spent_usd: self.spent_usd,
            limit_usd: self.limit_usd.unwrap_or_default(),
        }
    }
}

/// One direction's current (or last) session.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionUsage {
    /// The direction is running.
    pub active: bool,
    pub seconds: f64,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct SessionsUsage {
    pub incoming: SessionUsage,
    pub outgoing: SessionUsage,
}

/// Usage over a period (today, this month).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeriodUsage {
    pub incoming_seconds: f64,
    pub outgoing_seconds: f64,
    pub cost_usd: f64,
}

impl PeriodUsage {
    fn add(&mut self, day: &DayRecord) {
        self.incoming_seconds += day.incoming.seconds;
        self.outgoing_seconds += day.outgoing.seconds;
        self.cost_usd = cost_usd(self.incoming_seconds + self.outgoing_seconds);
    }
}

/// One day of the history.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayUsage {
    /// Local calendar day, `YYYY-MM-DD`.
    pub date: NaiveDate,
    pub incoming_seconds: f64,
    pub outgoing_seconds: f64,
    pub cost_usd: f64,
}

/// Everything the Usage screen shows (payload of the `usage-updated` event).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSnapshot {
    pub price_per_minute_usd: f64,
    pub session: SessionsUsage,
    pub today: PeriodUsage,
    pub month: PeriodUsage,
    /// Days with usage, newest first (at most [`HISTORY_DAYS`]).
    pub days: Vec<DayUsage>,
    pub cap: CapStatus,
}

/// What [`UsageMeter::record_sent`] found.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CapCheck {
    pub status: CapStatus,
    /// The cap was just reached: the running directions must stop (reported once per crossing).
    pub trip: bool,
}

#[derive(Debug, Clone, Copy, Default)]
struct Session {
    active: bool,
    seconds: f64,
    /// The pipeline's cumulative `AudioSent` seconds already counted.
    reported: f64,
    /// The last day this session was counted on (in that day's `sessions`).
    counted_on: Option<NaiveDate>,
}

struct MeterState {
    file: UsageFile,
    cap: SpendingCapSettings,
    sessions: [Session; 2],
    /// Changed since the last flush.
    dirty: bool,
    /// The running directions were told to stop for the cap; cleared once it no longer applies.
    tripped: bool,
}

/// Meters both directions and enforces the spending cap. See the [module docs](self).
pub struct UsageMeter {
    clock: Arc<dyn Clock>,
    /// `None`: in memory only (tests).
    path: Option<PathBuf>,
    state: Mutex<MeterState>,
    /// Held while writing, so concurrent flushes land in order.
    writing: Mutex<()>,
}

fn index(direction: Direction) -> usize {
    match direction {
        Direction::Incoming => 0,
        Direction::Outgoing => 1,
    }
}

fn day_direction(day: &mut DayRecord, direction: Direction) -> &mut DirectionDay {
    match direction {
        Direction::Incoming => &mut day.incoming,
        Direction::Outgoing => &mut day.outgoing,
    }
}

impl UsageMeter {
    /// A meter with the history in `path` (loaded now, see [`file::load`]); `None` keeps it in
    /// memory only.
    pub fn new(path: Option<PathBuf>, clock: Arc<dyn Clock>, cap: SpendingCapSettings) -> Self {
        let file = path.as_deref().map(file::load).unwrap_or_default();
        Self {
            clock,
            path,
            state: Mutex::new(MeterState {
                file,
                cap,
                sessions: [Session::default(); 2],
                dirty: false,
                tripped: false,
            }),
            writing: Mutex::new(()),
        }
    }

    fn lock(&self) -> MutexGuard<'_, MeterState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn today(&self) -> NaiveDate {
        self.clock.now().date_naive()
    }

    /// Applies new cap settings (when the settings are saved).
    pub fn set_cap(&self, cap: SpendingCapSettings) {
        self.lock().cap = cap;
    }

    /// A direction began starting: its session counter starts over.
    pub fn begin_session(&self, direction: Direction) {
        self.lock().sessions[index(direction)] = Session {
            active: true,
            ..Session::default()
        };
    }

    /// A direction stopped (or failed to start). Its last session stays visible.
    pub fn end_session(&self, direction: Direction) {
        self.lock().sessions[index(direction)].active = false;
    }

    /// Counts a pipeline's report of `total_seconds` of audio sent since it started (cumulative,
    /// so only what is new is added, to today's local day), and checks the cap.
    pub fn record_sent(&self, direction: Direction, total_seconds: f64) -> CapCheck {
        let today = self.today();
        let mut state = self.lock();
        let session = &mut state.sessions[index(direction)];
        let delta = (total_seconds - session.reported).max(0.0);
        session.reported = session.reported.max(total_seconds);
        if delta > 0.0 {
            session.seconds += delta;
            // A session spanning midnight counts on both days.
            let new_day = session.counted_on != Some(today);
            session.counted_on = Some(today);
            let day = state.file.days.entry(today).or_default();
            let usage = day_direction(day, direction);
            if new_day {
                usage.sessions += 1;
            }
            usage.seconds += delta;
            state.dirty = true;
        }
        let status = cap_status(&state, today);
        let trip = if status.enforced() {
            !std::mem::replace(&mut state.tripped, true)
        } else {
            state.tripped = false;
            false
        };
        CapCheck { status, trip }
    }

    /// The spending cap's status now.
    pub fn cap_status(&self) -> CapStatus {
        let today = self.today();
        cap_status(&self.lock(), today)
    }

    /// Refuses to start a direction while the cap is reached (and not overridden today).
    pub fn check_start(&self) -> AppResult<()> {
        let status = self.cap_status();
        if status.enforced() {
            Err(status.error())
        } else {
            Ok(())
        }
    }

    /// The user chose to keep translating over the cap for the rest of today.
    pub fn override_today(&self) {
        let today = self.today();
        let mut state = self.lock();
        state.file.cap_override_date = Some(today);
        state.tripped = false;
        state.dirty = true;
    }

    /// Everything the Usage screen shows, now.
    pub fn snapshot(&self) -> UsageSnapshot {
        let today = self.today();
        let state = self.lock();
        let session = |direction: Direction| {
            let session = state.sessions[index(direction)];
            SessionUsage {
                active: session.active,
                seconds: session.seconds,
                cost_usd: cost_usd(session.seconds),
            }
        };
        let mut today_usage = PeriodUsage::default();
        if let Some(day) = state.file.days.get(&today) {
            today_usage.add(day);
        }
        let mut month = PeriodUsage::default();
        for day in state
            .file
            .days
            .range(month_start(today)..=today)
            .map(|(_, day)| day)
        {
            month.add(day);
        }
        let days = state
            .file
            .days
            .iter()
            .rev()
            .filter(|(_, day)| day.seconds() > 0.0)
            .take(HISTORY_DAYS)
            .map(|(date, day)| DayUsage {
                date: *date,
                incoming_seconds: day.incoming.seconds,
                outgoing_seconds: day.outgoing.seconds,
                cost_usd: cost_usd(day.seconds()),
            })
            .collect();
        UsageSnapshot {
            price_per_minute_usd: PRICE_PER_MINUTE_USD,
            session: SessionsUsage {
                incoming: session(Direction::Incoming),
                outgoing: session(Direction::Outgoing),
            },
            today: today_usage,
            month,
            days,
            cap: cap_status(&state, today),
        }
    }

    /// Writes the history to disk (atomically) if it changed. Blocking: run it off the audio
    /// threads.
    pub fn flush(&self) -> std::io::Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let _writing = self.writing.lock().unwrap_or_else(PoisonError::into_inner);
        let json = {
            let mut state = self.lock();
            if !state.dirty {
                return Ok(());
            }
            state.dirty = false;
            file::to_json(&state.file)
        };
        let result = json.and_then(|json| file::write(path, &json));
        if result.is_err() {
            // Try again on the next flush.
            self.lock().dirty = true;
        }
        result
    }
}

fn month_start(date: NaiveDate) -> NaiveDate {
    date.with_day(1).unwrap_or(date)
}

fn cap_status(state: &MeterState, today: NaiveDate) -> CapStatus {
    let SpendingCapSettings {
        daily_usd,
        monthly_usd,
    } = state.cap;
    let today_usd = state
        .file
        .days
        .get(&today)
        .map_or(0.0, |day| cost_usd(day.seconds()));
    let month_usd = cost_usd(
        state
            .file
            .days
            .range(month_start(today)..=today)
            .map(|(_, day)| day.seconds())
            .sum(),
    );
    let mut closest: Option<(CapPeriod, f64, f64, f64)> = None;
    for (period, cap, spent) in [
        (CapPeriod::Daily, daily_usd, today_usd),
        (CapPeriod::Monthly, monthly_usd, month_usd),
    ] {
        let Some(cap) = cap.filter(|cap| *cap > 0.0) else {
            continue;
        };
        let fraction = spent / cap;
        if closest.is_none_or(|(_, best, _, _)| fraction > best) {
            closest = Some((period, fraction, spent, cap));
        }
    }
    let overridden_today = state.file.cap_override_date == Some(today);
    match closest {
        None => CapStatus {
            daily_usd,
            monthly_usd,
            level: CapLevel::Off,
            period: None,
            spent_usd: today_usd,
            limit_usd: None,
            overridden_today,
        },
        Some((period, fraction, spent, cap)) => CapStatus {
            daily_usd,
            monthly_usd,
            level: if fraction >= 1.0 {
                CapLevel::Reached
            } else if fraction >= WARNING_FRACTION {
                CapLevel::Warning
            } else {
                CapLevel::Ok
            },
            period: Some(period),
            spent_usd: spent,
            limit_usd: Some(cap),
            overridden_today,
        },
    }
}

/// What a [`metered_sink`] tells its owner.
#[derive(Debug, Clone, PartialEq)]
pub enum UsageNotice {
    /// The usage changed (the panel's snapshot is stale).
    Updated,
    /// A direction stopped: a good moment to write the history to disk.
    SessionEnded,
    /// The spending cap was just reached: stop every running direction.
    CapReached(CapStatus),
}

/// Handles a [`UsageNotice`]. Called from the pipelines' threads: it must not block.
pub type UsageNotify = Arc<dyn Fn(UsageNotice) + Send + Sync>;

/// Meters `direction`'s pipeline events into `meter` before forwarding them to `events`:
/// `connecting` begins a session, `AudioSent` counts audio (and may reach the cap), `idle` /
/// `error` end it.
pub fn metered_sink(
    meter: Arc<UsageMeter>,
    direction: Direction,
    events: EventSink,
    notify: UsageNotify,
) -> EventSink {
    Arc::new(move |event| {
        match &event {
            PipelineEvent::Status(status) => match status.state {
                PipelineState::Connecting => {
                    meter.begin_session(direction);
                    notify(UsageNotice::Updated);
                }
                PipelineState::Idle | PipelineState::Error => {
                    meter.end_session(direction);
                    notify(UsageNotice::SessionEnded);
                }
                _ => {}
            },
            PipelineEvent::AudioSent { seconds } => {
                let check = meter.record_sent(direction, *seconds);
                notify(UsageNotice::Updated);
                if check.trip {
                    tracing::warn!(
                        period = ?check.status.period,
                        spent_usd = check.status.spent_usd,
                        limit_usd = check.status.limit_usd,
                        "spending cap reached: stopping the translations"
                    );
                    notify(UsageNotice::CapReached(check.status));
                }
            }
            _ => {}
        }
        events(event);
    })
}

#[cfg(test)]
mod tests;
