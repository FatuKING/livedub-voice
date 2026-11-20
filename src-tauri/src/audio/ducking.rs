//! Ducking: lowering the original audio of the captured apps while the incoming direction plays
//! their translation.
//!
//! # How
//!
//! The captured apps' own audio sessions are turned down through `ISimpleAudioVolume` (the
//! per-app volume of the Windows volume mixer) to `original × level`, and turned back up when the
//! direction stops. The original keeps playing straight from the app to its device, so ducking
//! adds no latency and LiveDub Voice never plays the original itself.
//!
//! Process loopback captures each app's audio **after** its session volume (measured: see the
//! `session_volume_scales_the_loopback_capture` hardware test), so ducking alone would also
//! starve the translator. The capture is `f32` and the session volume is a plain gain applied in
//! floating point, so the pipeline multiplies the captured audio by [`capture_gain`] (`1 / level`)
//! and the translator hears the app at its normal volume. That is why the level never goes below
//! [`MIN_DUCK_LEVEL`] (−46 dB, inaudible under the translation): at zero there would be nothing
//! left to translate.
//!
//! # Which sessions
//!
//! The same ones process loopback captures: every session of the chosen app's process tree, or,
//! for the whole system, every session on every playback device except LiveDub Voice's own
//! (system sounds included). Sessions are rescanned every [`SCAN_INTERVAL`], so an app that starts
//! playing later (a new browser tab, a game launching) is ducked too.
//!
//! # Restoring
//!
//! Windows remembers an app's volume across restarts, so a volume left ducked would stay low for
//! good. Every ducked session is written to a small journal (`ducking.json`, next to the
//! settings) *before* its volume changes, and removed once restored. On stop the ducker restores
//! every session it lowered; whatever it could not restore (LiveDub Voice crashed, or the app was
//! closed and its session is gone) stays in the journal and is restored by [`recover`] the next
//! time LiveDub Voice starts and the app has a session again. A volume the user changed by hand
//! while ducked is theirs: it is never touched again.
//!
//! The bookkeeping ([`DuckState`]) is pure and unit tested; only reading and setting session
//! volumes talks to Windows.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::JoinHandle;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::apps::ProcessTable;
use super::loopback::LoopbackTarget;
use crate::error::{AppError, AppResult};

/// Name of the journal file inside the app config dir.
pub const JOURNAL_FILE_NAME: &str = "ducking.json";

/// Lowest level the original is ducked to (−46 dB): the capture is compensated by `1 / level`,
/// so it must stay above zero (see the module docs).
pub const MIN_DUCK_LEVEL: f32 = 0.005;

/// Whether process loopback captures an app after its session volume (measured on Windows 11;
/// see the module docs). When it does, the capture is compensated by [`capture_gain`].
pub const CAPTURE_FOLLOWS_SESSION_VOLUME: bool = true;

/// How often the sessions are rescanned for apps that started playing. Until a new session is
/// ducked, the translator hears it amplified by the capture gain (clipped at full scale), so this
/// stays short; a scan is a process snapshot plus the session list, well under a millisecond of
/// CPU.
pub const SCAN_INTERVAL: Duration = Duration::from_millis(250);

/// How long a lowered session volume takes to reach the captured audio (engine buffers), before
/// the capture gain goes up to match.
const GAIN_SETTLE: Duration = Duration::from_millis(60);

/// Difference between a session's volume and the one we set beyond which the user (or the app)
/// changed it.
const VOLUME_TOLERANCE: f32 = 0.002;

/// The session volume factor for an original-audio volume in percent (0–100), never below
/// [`MIN_DUCK_LEVEL`].
pub fn duck_level(original_volume_percent: u8) -> f32 {
    (f32::from(original_volume_percent.min(100)) / 100.0).max(MIN_DUCK_LEVEL)
}

/// What the captured audio is multiplied by so the translator hears the app at its normal volume
/// while its session is ducked to `level` (1 when the capture is not affected by the session
/// volume).
pub fn capture_gain(level: f32) -> f32 {
    if CAPTURE_FOLLOWS_SESSION_VOLUME {
        1.0 / level.clamp(MIN_DUCK_LEVEL, 1.0)
    } else {
        1.0
    }
}

/// The session volume an app at `original` gets while ducked to `level`.
pub fn ducked_volume(original: f32, level: f32) -> f32 {
    (original * level).clamp(0.0, 1.0)
}

/// Whether a session's volume is no longer the one we set.
fn changed_since(current: f32, set: f32) -> bool {
    (current - set).abs() > VOLUME_TOLERANCE
}

/// An audio session's volume, as read from Windows.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionVolume {
    /// The app's session identifier (`GetSessionIdentifier`): the same for every session of the
    /// app on the same device, and across restarts of the app.
    pub session_id: String,
    /// This session's identifier (`GetSessionInstanceIdentifier`), unique per session.
    pub instance_id: String,
    /// Process that owns the session (0 for system sounds).
    pub pid: u32,
    /// `ISimpleAudioVolume` master volume, 0–1.
    pub volume: f32,
}

/// A session the ducker lowered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DuckedSession {
    pub session_id: String,
    pub instance_id: String,
    /// Its volume before ducking.
    pub original: f32,
    /// The volume we set.
    pub ducked: f32,
    /// The user changed the volume while it was ducked: leave it alone (not journaled).
    #[serde(skip)]
    pub released: bool,
}

/// A volume to set: `session` indexes the sessions passed in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VolumeChange {
    pub session: usize,
    pub volume: f32,
}

/// The sessions the ducker lowered and has not restored yet (the journal's contents).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DuckState {
    pub sessions: Vec<DuckedSession>,
}

impl DuckState {
    /// Ducks `sessions` (the ones in scope) to `level`. Returns the volumes to set.
    ///
    /// - A session already ducked keeps its original; if `level` changed, it is re-ducked.
    /// - A new session of an app whose previous session we ducked and lost (the app restarted:
    ///   Windows gave the new session the ducked volume) is taken over, original included.
    /// - Any other session is new: its current volume is the original.
    /// - A ducked session whose volume changed since we set it was changed by the user: it is
    ///   released and never touched again.
    pub fn duck(&mut self, sessions: &[SessionVolume], level: f32) -> Vec<VolumeChange> {
        let mut changes = Vec::new();
        for (index, session) in sessions.iter().enumerate() {
            let known = self
                .sessions
                .iter()
                .position(|ducked| ducked.instance_id == session.instance_id)
                .or_else(|| {
                    // Taken over only when the lost session's own instance is not still live.
                    self.sessions.iter().position(|ducked| {
                        !ducked.released
                            && ducked.session_id == session.session_id
                            && !changed_since(session.volume, ducked.ducked)
                            && !sessions
                                .iter()
                                .any(|other| other.instance_id == ducked.instance_id)
                    })
                });
            match known {
                Some(position) => {
                    let ducked = &mut self.sessions[position];
                    if ducked.released {
                        continue;
                    }
                    ducked.instance_id.clone_from(&session.instance_id);
                    if changed_since(session.volume, ducked.ducked) {
                        tracing::info!(
                            session = %ducked.session_id,
                            "a ducked app's volume was changed by hand; leaving it alone"
                        );
                        ducked.released = true;
                        continue;
                    }
                    let target = ducked_volume(ducked.original, level);
                    if changed_since(target, ducked.ducked) {
                        ducked.ducked = target;
                        changes.push(VolumeChange {
                            session: index,
                            volume: target,
                        });
                    }
                }
                None => {
                    let target = ducked_volume(session.volume, level);
                    self.sessions.push(DuckedSession {
                        session_id: session.session_id.clone(),
                        instance_id: session.instance_id.clone(),
                        original: session.volume,
                        ducked: target,
                        released: false,
                    });
                    changes.push(VolumeChange {
                        session: index,
                        volume: target,
                    });
                }
            }
        }
        changes
    }

    /// Restores every ducked session found in `sessions` (all sessions, in scope or not).
    /// Returns the volumes to set.
    ///
    /// A session is found by its instance, or else by its app's session identifier (the app
    /// restarted). It gets its original volume back unless the user changed it since. Sessions
    /// not found stay in the state (to be restored later); the rest are forgotten.
    pub fn restore(&mut self, sessions: &[SessionVolume]) -> Vec<VolumeChange> {
        let mut changes = Vec::new();
        self.sessions.retain(|ducked| {
            if ducked.released {
                return false;
            }
            let by_instance: Vec<usize> = sessions
                .iter()
                .enumerate()
                .filter(|(_, session)| session.instance_id == ducked.instance_id)
                .map(|(index, _)| index)
                .collect();
            let found = if by_instance.is_empty() {
                sessions
                    .iter()
                    .enumerate()
                    .filter(|(_, session)| session.session_id == ducked.session_id)
                    .map(|(index, _)| index)
                    .collect()
            } else {
                by_instance
            };
            if found.is_empty() {
                return true;
            }
            for index in found {
                if changed_since(sessions[index].volume, ducked.ducked) {
                    tracing::info!(
                        session = %ducked.session_id,
                        "a ducked app's volume was changed by hand; not restoring it"
                    );
                } else if !changes
                    .iter()
                    .any(|change: &VolumeChange| change.session == index)
                {
                    changes.push(VolumeChange {
                        session: index,
                        volume: ducked.original,
                    });
                }
            }
            false
        });
        changes
    }

    /// The sessions worth journaling (not released).
    fn journaled(&self) -> Vec<&DuckedSession> {
        self.sessions
            .iter()
            .filter(|ducked| !ducked.released)
            .collect()
    }
}

/// Which sessions are ducked: the same audio process loopback captures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DuckScope {
    /// Sessions of this process and its descendants.
    ProcessTree(u32),
    /// Every session except those of this process tree (and system sounds included).
    AllExcept(u32),
}

impl From<LoopbackTarget> for DuckScope {
    fn from(target: LoopbackTarget) -> Self {
        match target {
            LoopbackTarget::ProcessTree(pid) => Self::ProcessTree(pid),
            LoopbackTarget::ExcludeProcessTree(pid) => Self::AllExcept(pid),
        }
    }
}

impl DuckScope {
    /// Whether the session of process `pid` is in scope.
    pub fn contains(self, table: &ProcessTable, pid: u32) -> bool {
        match self {
            Self::ProcessTree(root) => table.is_in_tree_of(pid, root),
            Self::AllExcept(own) => pid == 0 || !table.is_in_tree_of(pid, own),
        }
    }
}

/// Reads the journal; a missing or unreadable file is an empty state (the latter logged).
pub fn load_journal(path: &Path) -> DuckState {
    match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|error| {
            tracing::warn!(path = %path.display(), %error, "unreadable ducking journal, ignoring it");
            DuckState::default()
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => DuckState::default(),
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "could not read the ducking journal");
            DuckState::default()
        }
    }
}

/// Writes the journal atomically, or removes it when nothing is left to restore.
pub fn save_journal(path: &Path, state: &DuckState) -> io::Result<()> {
    let sessions = state.journaled();
    if sessions.is_empty() {
        return match fs::remove_file(path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        };
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string_pretty(&serde_json::json!({ "sessions": sessions }))
        .map_err(io::Error::other)?;
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let result = (|| {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(json.as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn save_or_log(journal: Option<&Path>, state: &DuckState) {
    if let Some(path) = journal
        && let Err(error) = save_journal(path, state)
    {
        tracing::warn!(path = %path.display(), %error, "could not write the ducking journal");
    }
}

/// Restores the volumes a previous run left ducked (see the module docs). Blocking (COM).
///
/// Returns how many sessions were restored; sessions whose app has no session now stay in the
/// journal for next time.
pub fn recover(journal: &Path) -> AppResult<usize> {
    let mut state = load_journal(journal);
    if state.sessions.is_empty() {
        return Ok(0);
    }
    let _com = imp::com()?;
    let sessions = imp::sessions()?;
    let restored = apply(&sessions, &state.restore(&infos(&sessions)));
    save_or_log(Some(journal), &state);
    tracing::info!(
        restored,
        pending = state.sessions.len(),
        "restored app volumes left ducked by a previous run"
    );
    Ok(restored)
}

fn infos(sessions: &[imp::Session]) -> Vec<SessionVolume> {
    sessions
        .iter()
        .map(|session| session.info.clone())
        .collect()
}

/// Sets the volumes, returning how many were set.
fn apply(sessions: &[imp::Session], changes: &[VolumeChange]) -> usize {
    let mut applied = 0;
    for change in changes {
        let session = &sessions[change.session];
        match session.set_volume(change.volume) {
            Ok(()) => applied += 1,
            Err(error) => tracing::warn!(
                session = %session.info.session_id,
                %error,
                "could not change an app's volume"
            ),
        }
    }
    applied
}

/// The current capture gain (see [`capture_gain`]), shared with the pipeline.
#[derive(Debug, Clone)]
pub struct SharedGain(Arc<AtomicU32>);

impl Default for SharedGain {
    fn default() -> Self {
        Self::new(1.0)
    }
}

impl SharedGain {
    pub fn new(gain: f32) -> Self {
        Self(Arc::new(AtomicU32::new(gain.to_bits())))
    }

    pub fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }

    pub fn set(&self, gain: f32) {
        self.0.store(gain.to_bits(), Ordering::Relaxed);
    }
}

enum Command {
    Level(f32),
    Stop,
}

/// Ducks the apps in a [`DuckScope`] on its own thread until [`stop`](Self::stop) (or drop),
/// which restores them.
pub struct Ducker {
    commands: mpsc::Sender<Command>,
    thread: Option<JoinHandle<()>>,
    gain: SharedGain,
}

impl Ducker {
    /// Starts ducking `scope` to `original_volume_percent`. Blocking: returns once the sessions
    /// playing now are ducked. `journal` is where the ducked volumes are remembered until
    /// restored (`None`: nowhere, for tests).
    pub fn start(
        scope: DuckScope,
        original_volume_percent: u8,
        journal: Option<PathBuf>,
    ) -> AppResult<Self> {
        let level = duck_level(original_volume_percent);
        let gain = SharedGain::new(1.0);
        let (commands, receiver) = mpsc::channel();
        let (started_tx, started_rx) = mpsc::channel();
        let thread_gain = gain.clone();
        let thread = std::thread::Builder::new()
            .name("livedub-ducking".into())
            .spawn(move || {
                run(scope, level, journal, receiver, thread_gain, |result| {
                    let _ = started_tx.send(result);
                });
            })
            .map_err(|error| {
                AppError::Internal(format!("could not start the ducking thread: {error}"))
            })?;
        let mut ducker = Self {
            commands,
            thread: Some(thread),
            gain,
        };
        match started_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(ducker),
            Ok(Err(error)) => {
                ducker.stop();
                Err(error)
            }
            Err(_) => {
                ducker.stop();
                Err(AppError::Audio(
                    "could not lower the original audio: Windows did not answer".into(),
                ))
            }
        }
    }

    /// The gain the captured audio must be multiplied by (see [`capture_gain`]).
    pub fn gain(&self) -> SharedGain {
        self.gain.clone()
    }

    /// Changes the original-audio volume while ducking.
    pub fn set_original_volume(&self, percent: u8) {
        let _ = self.commands.send(Command::Level(duck_level(percent)));
    }

    /// Restores the ducked apps and waits for the thread. Blocking.
    pub fn stop(&mut self) {
        let _ = self.commands.send(Command::Stop);
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::error!("ducking thread panicked");
        }
    }
}

impl Drop for Ducker {
    fn drop(&mut self) {
        self.stop();
    }
}

fn run(
    scope: DuckScope,
    mut level: f32,
    journal: Option<PathBuf>,
    commands: mpsc::Receiver<Command>,
    gain: SharedGain,
    started: impl FnOnce(AppResult<()>),
) {
    let _com = match imp::com() {
        Ok(com) => com,
        Err(error) => {
            started(Err(error));
            return;
        }
    };
    // Volumes a previous run left ducked are still in the journal: they are taken over (an app
    // still at its ducked volume keeps its true original) and restored with the rest at stop.
    let mut state = journal.as_deref().map(load_journal).unwrap_or_default();
    let scan = |state: &mut DuckState, level: f32| -> AppResult<()> {
        let table = super::apps::process_table()?;
        let sessions: Vec<imp::Session> = imp::sessions()?
            .into_iter()
            .filter(|session| scope.contains(&table, session.info.pid))
            .collect();
        let changes = state.duck(&infos(&sessions), level);
        if !changes.is_empty() {
            // Journal first: a crash right after must still find what to restore.
            save_or_log(journal.as_deref(), state);
            let applied = apply(&sessions, &changes);
            tracing::info!(applied, level, "ducked the original audio");
        }
        Ok(())
    };

    let first = scan(&mut state, level);
    gain.set(capture_gain(level));
    tracing::info!(?scope, level, gain = gain.get(), "ducking started");
    started(first);

    loop {
        match commands.recv_timeout(SCAN_INTERVAL) {
            Ok(Command::Level(new)) => {
                // Captured audio already in flight still has the old volume: change the gain
                // on the side where it can only come out quieter, never amplified past it.
                if new < level {
                    if let Err(error) = scan(&mut state, new) {
                        tracing::debug!(%error, "ducking rescan failed");
                    }
                    std::thread::sleep(GAIN_SETTLE);
                    gain.set(capture_gain(new));
                } else {
                    gain.set(capture_gain(new));
                }
                level = new;
            }
            Ok(Command::Stop) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {}
        }
        if let Err(error) = scan(&mut state, level) {
            tracing::debug!(%error, "ducking rescan failed");
        }
    }

    match imp::sessions() {
        Ok(sessions) => {
            let restored = apply(&sessions, &state.restore(&infos(&sessions)));
            tracing::info!(
                restored,
                pending = state.sessions.len(),
                "ducking stopped, original audio restored"
            );
        }
        Err(error) => tracing::warn!(%error, "could not restore the ducked apps"),
    }
    save_or_log(journal.as_deref(), &state);
    gain.set(1.0);
}

#[cfg(windows)]
mod imp {
    use windows::Win32::Media::Audio::{
        AudioSessionStateExpired, DEVICE_STATE_ACTIVE, IAudioSessionControl2,
        IAudioSessionManager2, IMMDeviceEnumerator, ISimpleAudioVolume, MMDeviceEnumerator,
        eRender,
    };
    use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance, CoTaskMemFree};
    use windows::core::{GUID, Interface, PWSTR};

    use super::SessionVolume;
    use crate::audio::stream::audio_error;
    use crate::audio::stream::win::ComGuard;
    use crate::error::AppResult;

    const READ: &str = "could not read the apps' volumes";
    const SET: &str = "could not change an app's volume";

    /// Event context of our volume changes ("LiveDub Voice ducking"), so a listener can tell
    /// them apart.
    const CONTEXT: GUID = GUID::from_u128(0x6c1d_ab0e_5d1e_4b6e_9f4a_1d0c_e0d1_0b01);

    pub(super) fn com() -> AppResult<ComGuard> {
        ComGuard::new()
    }

    /// A session and its volume control.
    pub(super) struct Session {
        pub(super) info: SessionVolume,
        volume: ISimpleAudioVolume,
    }

    impl Session {
        pub(super) fn set_volume(&self, volume: f32) -> AppResult<()> {
            // SAFETY: plain COM call on a live interface; the context outlives the call.
            unsafe {
                self.volume
                    .SetMasterVolume(volume.clamp(0.0, 1.0), &CONTEXT)
            }
            .map_err(|e| audio_error(SET, Some(e.code().0), &e))
        }
    }

    fn take_string(text: PWSTR) -> String {
        if text.is_null() {
            return String::new();
        }
        // SAFETY: a non-null string allocated by COM for us, freed once here.
        unsafe {
            let value = text.to_string().unwrap_or_default();
            CoTaskMemFree(Some(text.0 as *const _));
            value
        }
    }

    /// The non-expired sessions of every active playback device. A device or session that
    /// fails to read is skipped (it may have just gone away).
    pub(super) fn sessions() -> AppResult<Vec<Session>> {
        // SAFETY: COM is initialized on this thread (callers hold a ComGuard); every interface
        // is used while alive.
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                    .map_err(|e| audio_error(READ, Some(e.code().0), &e))?;
            let devices = enumerator
                .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)
                .map_err(|e| audio_error(READ, Some(e.code().0), &e))?;
            let count = devices
                .GetCount()
                .map_err(|e| audio_error(READ, Some(e.code().0), &e))?;
            let mut sessions = Vec::new();
            for index in 0..count {
                let read = (|| -> windows::core::Result<()> {
                    let device = devices.Item(index)?;
                    let manager: IAudioSessionManager2 = device.Activate(CLSCTX_ALL, None)?;
                    let list = manager.GetSessionEnumerator()?;
                    for session_index in 0..list.GetCount()? {
                        let Ok(control) = list.GetSession(session_index) else {
                            continue;
                        };
                        let session = (|| -> windows::core::Result<Option<Session>> {
                            if control.GetState()? == AudioSessionStateExpired {
                                return Ok(None);
                            }
                            let control2: IAudioSessionControl2 = control.cast()?;
                            let volume: ISimpleAudioVolume = control.cast()?;
                            Ok(Some(Session {
                                info: SessionVolume {
                                    session_id: take_string(control2.GetSessionIdentifier()?),
                                    instance_id: take_string(
                                        control2.GetSessionInstanceIdentifier()?,
                                    ),
                                    // Multi-process sessions report no single process.
                                    pid: control2.GetProcessId().unwrap_or(0),
                                    volume: volume.GetMasterVolume()?,
                                },
                                volume,
                            }))
                        })();
                        match session {
                            Ok(Some(session)) => sessions.push(session),
                            Ok(None) => {}
                            Err(error) => tracing::debug!(%error, "skipping an audio session"),
                        }
                    }
                    Ok(())
                })();
                if let Err(error) = read {
                    tracing::debug!(%error, index, "skipping the sessions of a playback device");
                }
            }
            Ok(sessions)
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::SessionVolume;
    use crate::error::{AppError, AppResult};

    pub(super) struct Session {
        pub(super) info: SessionVolume,
    }

    impl Session {
        pub(super) fn set_volume(&self, _: f32) -> AppResult<()> {
            Err(AppError::Audio(
                "ducking is only supported on Windows".into(),
            ))
        }
    }

    pub(super) fn com() -> AppResult<()> {
        Ok(())
    }

    pub(super) fn sessions() -> AppResult<Vec<Session>> {
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::apps::ProcessEntry;

    fn session(app: &str, instance: u32, volume: f32) -> SessionVolume {
        SessionVolume {
            session_id: format!("device|{app}"),
            instance_id: format!("device|{app}|{instance}"),
            pid: instance,
            volume,
        }
    }

    fn set(changes: &[VolumeChange]) -> Vec<(usize, f32)> {
        changes
            .iter()
            .map(|change| (change.session, (change.volume * 1000.0).round() / 1000.0))
            .collect()
    }

    #[test]
    fn levels_and_gain() {
        assert_eq!(duck_level(100), 1.0);
        assert_eq!(duck_level(20), 0.2);
        assert_eq!(duck_level(200), 1.0);
        // Zero would leave nothing to translate.
        assert_eq!(duck_level(0), MIN_DUCK_LEVEL);
        assert_eq!(capture_gain(1.0), 1.0);
        assert!((capture_gain(0.2) - 5.0).abs() < 1e-5);
        assert_eq!(capture_gain(0.0), 1.0 / MIN_DUCK_LEVEL);
        // Ducking keeps the user's own app volume as the reference.
        assert!((ducked_volume(0.5, 0.2) - 0.1).abs() < 1e-6);
        // Compensated capture: what the app plays at, times the gain, is the original.
        for percent in [0, 1, 20, 50, 100] {
            let level = duck_level(percent);
            assert!((0.8 * level * capture_gain(level) - 0.8).abs() < 1e-5);
        }
    }

    #[test]
    fn ducks_new_sessions_relative_to_their_volume() {
        let mut state = DuckState::default();
        let sessions = [session("discord", 1, 1.0), session("chrome", 2, 0.5)];

        let changes = state.duck(&sessions, 0.2);

        assert_eq!(set(&changes), [(0, 0.2), (1, 0.1)]);
        assert_eq!(state.sessions.len(), 2);
        assert_eq!(state.sessions[1].original, 0.5);

        // Nothing changed: nothing to do on the next scan.
        let ducked = [session("discord", 1, 0.2), session("chrome", 2, 0.1)];
        assert!(state.duck(&ducked, 0.2).is_empty());

        // A new level re-ducks from the originals.
        assert_eq!(set(&state.duck(&ducked, 0.5)), [(0, 0.5), (1, 0.25)]);
    }

    #[test]
    fn restore_gives_back_the_original_volume() {
        let mut state = DuckState::default();
        state.duck(&[session("discord", 1, 0.8)], 0.25);

        let changes = state.restore(&[session("other", 9, 1.0), session("discord", 1, 0.2)]);

        assert_eq!(set(&changes), [(1, 0.8)]);
        assert!(state.sessions.is_empty());
    }

    #[test]
    fn a_volume_changed_by_hand_is_left_alone() {
        let mut state = DuckState::default();
        state.duck(&[session("discord", 1, 1.0)], 0.2);

        // The user raised Discord in the volume mixer while ducked.
        let changes = state.duck(&[session("discord", 1, 0.6)], 0.2);
        assert!(changes.is_empty());
        // Never re-ducked, even when the level changes.
        assert!(state.duck(&[session("discord", 1, 0.6)], 0.5).is_empty());
        // Nor restored, nor journaled.
        assert!(state.journaled().is_empty());
        assert!(state.restore(&[session("discord", 1, 0.6)]).is_empty());
        assert!(state.sessions.is_empty());

        // Changed by hand between the last scan and the stop: not restored either.
        let mut state = DuckState::default();
        state.duck(&[session("chrome", 2, 1.0)], 0.2);
        assert!(state.restore(&[session("chrome", 2, 0.7)]).is_empty());
    }

    #[test]
    fn a_restarted_app_is_taken_over_with_its_true_original() {
        let mut state = DuckState::default();
        state.duck(&[session("discord", 1, 0.9)], 0.2);

        // Discord restarts: Windows gives its new session the ducked volume it remembered.
        let changes = state.duck(&[session("discord", 2, 0.18)], 0.2);

        assert!(changes.is_empty(), "not ducked twice");
        assert_eq!(state.sessions.len(), 1);
        assert_eq!(state.sessions[0].instance_id, "device|discord|2");
        assert_eq!(
            set(&state.restore(&[session("discord", 2, 0.18)])),
            [(0, 0.9)]
        );
    }

    #[test]
    fn a_second_session_of_the_same_app_is_ducked_on_its_own() {
        let mut state = DuckState::default();
        state.duck(&[session("chrome", 1, 1.0)], 0.2);

        // A second Chrome session at the ducked volume while the first one still plays: a new
        // session (its volume is its own), not a take-over.
        let both = [session("chrome", 1, 0.2), session("chrome", 2, 0.2)];
        let changes = state.duck(&both, 0.2);

        assert_eq!(set(&changes), [(1, 0.04)]);
        assert_eq!(state.sessions.len(), 2);
    }

    #[test]
    fn sessions_that_are_gone_wait_in_the_journal() {
        let mut state = DuckState::default();
        state.duck(&[session("discord", 1, 1.0), session("game", 2, 0.7)], 0.2);

        // At stop the game was closed: Discord is restored, the game waits.
        let changes = state.restore(&[session("discord", 1, 0.2)]);
        assert_eq!(set(&changes), [(0, 1.0)]);
        assert_eq!(state.sessions.len(), 1);

        // Next launch, the game plays again (new instance, remembered ducked volume).
        let changes = state.restore(&[session("game", 7, 0.14)]);
        assert_eq!(set(&changes), [(0, 0.7)]);
        assert!(state.sessions.is_empty());
    }

    #[test]
    fn journal_round_trips_and_disappears_when_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join(JOURNAL_FILE_NAME);
        let mut state = DuckState::default();
        state.duck(
            &[session("discord", 1, 0.9), session("chrome", 2, 1.0)],
            0.2,
        );
        // Released sessions are not journaled.
        state.duck(&[session("chrome", 2, 0.5)], 0.2);

        save_journal(&path, &state).unwrap();
        let loaded = load_journal(&path);

        assert_eq!(loaded.sessions.len(), 1);
        assert_eq!(loaded.sessions[0], state.sessions[0]);

        save_journal(&path, &DuckState::default()).unwrap();
        assert!(!path.exists());
        // Removing a journal that is not there is fine.
        save_journal(&path, &DuckState::default()).unwrap();
        assert!(load_journal(&path).sessions.is_empty());
    }

    #[test]
    fn unreadable_journal_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(JOURNAL_FILE_NAME);
        std::fs::write(&path, "not json").unwrap();

        assert!(load_journal(&path).sessions.is_empty());
    }

    #[test]
    fn scope_matches_what_loopback_captures() {
        let table = ProcessTable::new([
            ProcessEntry {
                pid: 100,
                parent_pid: 1,
                exe_name: "Discord.exe".into(),
            },
            ProcessEntry {
                pid: 101,
                parent_pid: 100,
                exe_name: "Discord.exe".into(),
            },
            ProcessEntry {
                pid: 200,
                parent_pid: 1,
                exe_name: "livedub-voice.exe".into(),
            },
            ProcessEntry {
                pid: 201,
                parent_pid: 200,
                exe_name: "msedgewebview2.exe".into(),
            },
            ProcessEntry {
                pid: 300,
                parent_pid: 1,
                exe_name: "chrome.exe".into(),
            },
        ]);

        let app = DuckScope::from(LoopbackTarget::ProcessTree(100));
        assert!(app.contains(&table, 101));
        assert!(!app.contains(&table, 300));
        assert!(!app.contains(&table, 0));

        let system = DuckScope::from(LoopbackTarget::ExcludeProcessTree(200));
        assert!(system.contains(&table, 101));
        assert!(system.contains(&table, 300));
        // System sounds are captured, so they are ducked too.
        assert!(system.contains(&table, 0));
        assert!(!system.contains(&table, 201));
    }

    #[test]
    fn shared_gain_is_shared() {
        let gain = SharedGain::default();
        let copy = gain.clone();
        assert_eq!(gain.get(), 1.0);
        copy.set(5.0);
        assert_eq!(gain.get(), 5.0);
    }
}

#[cfg(all(test, windows))]
#[allow(clippy::unwrap_used, clippy::zombie_processes)]
mod hardware_tests {
    use std::process::Child;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::audio::{LoopbackConfig, start_loopback_capture};

    /// A PowerShell child playing a quiet 440 Hz tone for `seconds`, and its audio session.
    fn tone_player(dir: &Path, seconds: u32) -> (Child, imp::Session) {
        let wav = dir.join("tone.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&wav, spec).unwrap();
        for i in 0..48_000 * seconds {
            let t = i as f32 / 48_000.0;
            let value = 0.25 * (std::f32::consts::TAU * 440.0 * t).sin();
            writer.write_sample((value * 32_767.0) as i16).unwrap();
        }
        writer.finalize().unwrap();

        let child = std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "(New-Object Media.SoundPlayer '{}').PlaySync()",
                    wav.display()
                ),
            ])
            .spawn()
            .unwrap();
        let pid = child.id();
        let deadline = Instant::now() + Duration::from_secs(6);
        loop {
            let found = imp::sessions()
                .unwrap()
                .into_iter()
                .find(|session| session.info.pid == pid);
            if let Some(session) = found {
                return (child, session);
            }
            assert!(Instant::now() < deadline, "no session for the tone player");
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn volume_of(pid: u32) -> f32 {
        imp::sessions()
            .unwrap()
            .into_iter()
            .find(|session| session.info.pid == pid)
            .unwrap()
            .info
            .volume
    }

    /// Plays a quiet 440 Hz tone from a PowerShell child process, captures it through process
    /// loopback and changes the child's session volume, printing the captured peak at each
    /// volume. Shows whether process loopback captures before or after the session volume (see
    /// [`CAPTURE_FOLLOWS_SESSION_VOLUME`]). Run with
    /// `cargo test --manifest-path src-tauri/Cargo.toml session_volume_scales_the_loopback_capture -- --ignored --nocapture`.
    ///
    /// Measured on Windows 11: peaks 0.24997 / 0.06249 / 0.00250 / 0.00125 at volumes
    /// 1 / 0.25 / 0.01 / 0.005, i.e. the capture is scaled exactly by the session volume, and
    /// compensating by `1 / volume` gives back 0.24997 at every volume.
    #[test]
    #[ignore = "plays a tone on the default output"]
    fn session_volume_scales_the_loopback_capture() {
        let _com = imp::com().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let (mut child, session) = tone_player(dir.path(), 12);
        let pid = child.id();
        println!("session {:?}", session.info);

        let (mut stream, mut consumer) =
            start_loopback_capture(LoopbackTarget::ProcessTree(pid), LoopbackConfig::default())
                .unwrap();
        let mut buffer = vec![0.0_f32; 48_000];
        let mut measure = |label: &str| {
            std::thread::sleep(Duration::from_millis(300));
            consumer.pop(&mut buffer);
            let mut peak = 0.0_f32;
            let start = Instant::now();
            while start.elapsed() < Duration::from_millis(800) {
                std::thread::sleep(Duration::from_millis(20));
                let read = consumer.pop(&mut buffer);
                peak = buffer[..read]
                    .iter()
                    .fold(peak, |peak, s| peak.max(s.abs()));
            }
            println!("{label}: captured peak {peak:.5}");
            peak
        };

        let original = session.info.volume;
        let full = measure("volume 1.0");
        session.set_volume(0.25).unwrap();
        let quarter = measure("volume 0.25");
        session.set_volume(0.01).unwrap();
        let hundredth = measure("volume 0.01");
        session.set_volume(MIN_DUCK_LEVEL).unwrap();
        let minimum = measure("volume MIN_DUCK_LEVEL");
        session.set_volume(original).unwrap();
        stream.stop();
        let _ = child.kill();

        println!(
            "compensated: 0.25 -> {:.5}, 0.01 -> {:.5}, min -> {:.5} (full {full:.5})",
            quarter * capture_gain(0.25),
            hundredth * capture_gain(0.01),
            minimum * capture_gain(MIN_DUCK_LEVEL)
        );
        assert!(full > 0.05, "the tone was not captured");
        for compensated in [
            quarter * capture_gain(0.25),
            hundredth * capture_gain(0.01),
            minimum * capture_gain(MIN_DUCK_LEVEL),
        ] {
            assert!(
                (compensated - full).abs() < full * 0.01,
                "{compensated} vs {full}"
            );
        }
    }

    /// Ducks a tone player with the real [`Ducker`], checks its session volume and the journal,
    /// restores it; then fakes a crash (volume left ducked, journal left behind) and checks that
    /// [`recover`] restores it. Run with
    /// `cargo test --manifest-path src-tauri/Cargo.toml ducker_ducks_and_restores_a_real_app -- --ignored --nocapture`.
    #[test]
    #[ignore = "plays a tone on the default output"]
    fn ducker_ducks_and_restores_a_real_app() {
        let _com = imp::com().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let journal = dir.path().join(JOURNAL_FILE_NAME);
        let (mut child, session) = tone_player(dir.path(), 8);
        let pid = child.id();
        let original = session.info.volume;

        let mut ducker =
            Ducker::start(DuckScope::ProcessTree(pid), 20, Some(journal.clone())).unwrap();
        let ducked = volume_of(pid);
        println!(
            "original {original}, ducked {ducked}, gain {}",
            ducker.gain().get()
        );
        assert!((ducked - original * 0.2).abs() < 0.01);
        assert!((ducker.gain().get() - 5.0).abs() < 1e-4);
        assert_eq!(load_journal(&journal).sessions.len(), 1);

        ducker.set_original_volume(50);
        std::thread::sleep(Duration::from_millis(200));
        assert!((volume_of(pid) - original * 0.5).abs() < 0.01);

        ducker.stop();
        assert!((volume_of(pid) - original).abs() < 0.01);
        assert!(!journal.exists());

        // A crash: ducked, journaled, never restored.
        let mut state = DuckState::default();
        let changes = state.duck(std::slice::from_ref(&session.info), 0.2);
        save_journal(&journal, &state).unwrap();
        let sessions = imp::sessions().unwrap();
        let index = sessions
            .iter()
            .position(|s| s.info.instance_id == session.info.instance_id)
            .unwrap();
        sessions[index].set_volume(changes[0].volume).unwrap();
        assert!((volume_of(pid) - original * 0.2).abs() < 0.01);

        assert_eq!(recover(&journal).unwrap(), 1);
        assert!((volume_of(pid) - original).abs() < 0.01);
        assert!(!journal.exists());
        let _ = child.kill();
    }
}
