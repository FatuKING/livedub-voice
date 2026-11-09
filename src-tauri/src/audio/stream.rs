//! Plumbing shared by the capture and playback streams: the dedicated stream thread and its
//! handle, and turning WASAPI failures into user-facing [`AppError::Audio`] messages.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, OnceLock};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::error::{AppError, AppResult};

/// How long `start` waits for the stream thread to open the device.
const START_TIMEOUT: Duration = Duration::from_secs(5);

/// How long a stream thread waits for a WASAPI buffer event before checking whether it should
/// stop. Events normally arrive every device period (~10 ms), so this only bounds how long
/// `stop()` can take when the device has gone quiet.
pub(crate) const EVENT_WAIT_MS: u32 = 100;

/// What a stream thread needs to know about its own lifetime.
#[derive(Clone)]
pub(crate) struct StreamControl {
    stop: Arc<AtomicBool>,
    failure: Arc<OnceLock<String>>,
}

impl StreamControl {
    /// Whether the owner asked the stream to stop.
    pub(crate) fn should_stop(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    /// Records why the stream stopped on its own (first failure wins).
    pub(crate) fn fail(&self, error: &AppError) {
        let _ = self.failure.set(error.to_string());
    }
}

/// Owner of a running stream thread. Stops it (and waits for it) on [`stop`](Self::stop) or drop.
pub struct StreamHandle {
    name: &'static str,
    control: StreamControl,
    thread: Option<JoinHandle<()>>,
}

impl StreamHandle {
    /// Stops the stream and waits for its thread to finish (at most ~[`EVENT_WAIT_MS`]).
    pub fn stop(&mut self) {
        self.control.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            if thread.join().is_err() {
                tracing::error!(stream = self.name, "audio stream thread panicked");
            } else {
                tracing::debug!(stream = self.name, "audio stream stopped");
            }
        }
    }

    /// Whether the stream thread is still running (it ends on `stop()` or on a device failure).
    pub fn is_running(&self) -> bool {
        self.thread
            .as_ref()
            .is_some_and(|thread| !thread.is_finished())
    }

    /// Why the stream stopped on its own, for example "the audio device was unplugged".
    pub fn failure(&self) -> Option<&str> {
        self.control.failure.get().map(String::as_str)
    }

    /// A cheap, thread-safe view of [`failure`](Self::failure) that outlives the borrow.
    pub fn status(&self) -> StreamStatus {
        StreamStatus(Arc::clone(&self.control.failure))
    }
}

/// Lock-free view of whether (and why) a stream failed; see [`StreamHandle::status`].
#[derive(Clone, Debug)]
pub struct StreamStatus(Arc<OnceLock<String>>);

impl StreamStatus {
    /// Why the stream stopped on its own, if it did.
    pub fn failure(&self) -> Option<&str> {
        self.0.get().map(String::as_str)
    }
}

impl Drop for StreamHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Spawns a dedicated stream thread and waits until it reports whether the device opened.
///
/// `body` runs on the new thread. It must send exactly one start result through `started`
/// (`Ok` with what the caller should get back, or the error that prevented starting) and then
/// run until `control.should_stop()`.
pub(crate) fn spawn_stream<T, F>(name: &'static str, body: F) -> AppResult<(StreamHandle, T)>
where
    T: Send + 'static,
    F: FnOnce(StreamControl, mpsc::SyncSender<AppResult<T>>) + Send + 'static,
{
    let control = StreamControl {
        stop: Arc::new(AtomicBool::new(false)),
        failure: Arc::new(OnceLock::new()),
    };
    let (started_tx, started_rx) = mpsc::sync_channel(1);
    let thread_control = control.clone();
    let thread = std::thread::Builder::new()
        .name(format!("livedub-{name}"))
        .spawn(move || body(thread_control, started_tx))
        .map_err(|error| {
            AppError::Internal(format!("could not start the {name} thread: {error}"))
        })?;

    let mut handle = StreamHandle {
        name,
        control,
        thread: Some(thread),
    };
    match started_rx.recv_timeout(START_TIMEOUT) {
        Ok(Ok(value)) => Ok((handle, value)),
        Ok(Err(error)) => Err(error),
        Err(RecvTimeoutError::Timeout) => {
            tracing::error!(stream = name, "audio device did not open in time");
            // The thread may be stuck inside Windows: ask it to stop but do not wait for it.
            handle.control.stop.store(true, Ordering::Relaxed);
            drop(handle.thread.take());
            Err(AppError::Audio(format!(
                "the {name} did not start in time; check that the device works in Windows sound settings"
            )))
        }
        Err(RecvTimeoutError::Disconnected) => Err(AppError::Internal(format!(
            "the {name} thread ended before starting"
        ))),
    }
}

/// `E_ACCESSDENIED`: typically the microphone privacy setting blocks desktop apps.
const E_ACCESSDENIED: u32 = 0x8007_0005;
/// `E_NOTFOUND` (`HRESULT_FROM_WIN32(ERROR_NOT_FOUND)`): no device with that id, or no default.
const E_NOTFOUND: u32 = 0x8007_0490;
const AUDCLNT_E_DEVICE_INVALIDATED: u32 = 0x8889_0004;
const AUDCLNT_E_UNSUPPORTED_FORMAT: u32 = 0x8889_0008;
const AUDCLNT_E_DEVICE_IN_USE: u32 = 0x8889_000A;
const AUDCLNT_E_ENDPOINT_CREATE_FAILED: u32 = 0x8889_000F;
const AUDCLNT_E_SERVICE_NOT_RUNNING: u32 = 0x8889_0010;
const AUDCLNT_E_RESOURCES_INVALIDATED: u32 = 0x8889_0026;

/// Plain-language reason for a Windows audio `HRESULT`, or `None` for codes the user cannot act
/// on (those keep a generic message; the code goes to the log).
pub fn describe_hresult(hresult: i32) -> Option<&'static str> {
    Some(match hresult as u32 {
        E_NOTFOUND => "the device was not found; it may have been unplugged or disabled",
        AUDCLNT_E_DEVICE_INVALIDATED | AUDCLNT_E_RESOURCES_INVALIDATED => {
            "the device was unplugged or disabled"
        }
        AUDCLNT_E_DEVICE_IN_USE => {
            "another app is using the device in exclusive mode; close it or turn off \"Allow applications to take exclusive control\" for the device"
        }
        AUDCLNT_E_UNSUPPORTED_FORMAT => "the device does not support this audio format",
        AUDCLNT_E_ENDPOINT_CREATE_FAILED => "Windows could not open the device",
        AUDCLNT_E_SERVICE_NOT_RUNNING => "the Windows Audio service is not running",
        E_ACCESSDENIED => {
            "Windows denied access; allow desktop apps to use the microphone in Settings > Privacy & security > Microphone"
        }
        _ => return None,
    })
}

/// Builds the user-facing error for a failed audio operation and logs the details.
///
/// `action` says what failed, for example "could not open the microphone".
pub(crate) fn audio_error(
    action: &str,
    hresult: Option<i32>,
    detail: &dyn std::fmt::Display,
) -> AppError {
    tracing::error!(action, hresult = hresult.map(|h| format!("{:#010x}", h as u32)), %detail, "audio stream error");
    let reason = hresult
        .and_then(describe_hresult)
        .unwrap_or("Windows reported an audio error (details are in the log)");
    AppError::Audio(format!("{action}: {reason}"))
}

#[cfg(windows)]
pub(crate) mod win {
    //! WASAPI helpers used by the stream threads.

    use wasapi::{AudioClient, Device, DeviceEnumerator, DeviceState, Direction, WasapiError};

    use super::audio_error;
    use crate::audio::format::{SampleFormat, StreamFormat};
    use crate::error::{AppError, AppResult};

    /// `RPC_E_CHANGED_MODE`: COM is already initialized on this thread with another apartment
    /// model; still usable, but must not be uninitialized by us.
    const RPC_E_CHANGED_MODE: i32 = 0x8001_0106_u32 as i32;

    /// Initializes COM (multithreaded apartment) for the stream thread and uninitializes it on
    /// drop, if this guard's call did the initialization.
    pub(crate) struct ComGuard {
        owns_init: bool,
    }

    impl ComGuard {
        pub(crate) fn new() -> AppResult<Self> {
            let hr = wasapi::initialize_mta();
            if hr.is_ok() {
                Ok(Self { owns_init: true })
            } else if hr.0 == RPC_E_CHANGED_MODE {
                Ok(Self { owns_init: false })
            } else {
                Err(audio_error(
                    "could not initialize Windows audio",
                    Some(hr.0),
                    &hr,
                ))
            }
        }
    }

    impl Drop for ComGuard {
        fn drop(&mut self) {
            if self.owns_init {
                wasapi::deinitialize();
            }
        }
    }

    /// The `HRESULT` inside a WASAPI error, if it came from Windows.
    pub(crate) fn hresult_of(error: &WasapiError) -> Option<i32> {
        match error {
            WasapiError::Windows(inner) | WasapiError::RegisterNotifications(inner) => {
                Some(inner.code().0)
            }
            _ => None,
        }
    }

    /// Maps a WASAPI error to a user-facing [`AppError::Audio`], logging the details.
    pub(crate) fn map_error(action: &str, error: WasapiError) -> AppError {
        match error {
            WasapiError::UnsupportedFormat | WasapiError::UnsupportedSubformat(_) => {
                audio_error(action, Some(0x8889_0008_u32 as i32), &error)
            }
            other => audio_error(action, hresult_of(&other), &other),
        }
    }

    /// Opens the device with `id`, or the default device of `direction` when `id` is `None`.
    /// Checks that the device is active and of the requested direction.
    pub(crate) fn open_device(
        id: Option<&str>,
        direction: Direction,
        action: &str,
    ) -> AppResult<Device> {
        let enumerator = DeviceEnumerator::new().map_err(|e| map_error(action, e))?;
        let device = match id {
            Some(id) => enumerator.get_device(id),
            None => enumerator.get_default_device(direction),
        }
        .map_err(|e| map_error(action, e))?;

        if device.get_direction() != direction {
            let expected = match direction {
                Direction::Capture => "an input (recording) device",
                Direction::Render => "an output (playback) device",
            };
            return Err(AppError::Audio(format!(
                "{action}: the selected device is not {expected}"
            )));
        }
        match device.get_state().map_err(|e| map_error(action, e))? {
            DeviceState::Active => Ok(device),
            state => {
                tracing::warn!(?state, "audio device is not active");
                Err(AppError::Audio(format!(
                    "{action}: the device is {}",
                    match state {
                        DeviceState::Disabled => "disabled in Windows sound settings",
                        DeviceState::Unplugged => "unplugged",
                        _ => "not present (it may have been removed)",
                    }
                )))
            }
        }
    }

    /// Reads the shared-mode mix format of an (uninitialized) audio client.
    pub(crate) fn mix_format(
        client: &AudioClient,
        action: &str,
    ) -> AppResult<(wasapi::WaveFormat, StreamFormat)> {
        let wave = client.get_mixformat().map_err(|e| map_error(action, e))?;
        let is_float = matches!(
            wave.get_subformat().map_err(|e| map_error(action, e))?,
            wasapi::SampleType::Float
        );
        let sample_format = SampleFormat::from_wave(is_float, wave.get_bitspersample())
            .ok_or_else(|| map_error(action, WasapiError::UnsupportedFormat))?;
        let format = StreamFormat {
            sample_rate: wave.get_samplespersec(),
            channels: wave.get_nchannels(),
            sample_format,
        };
        Ok((wave, format))
    }

    /// Shared-mode mix format of a device (what it delivers or expects in shared mode).
    pub(crate) fn device_mix_format(
        id: Option<&str>,
        direction: Direction,
        action: &str,
    ) -> AppResult<StreamFormat> {
        let _com = ComGuard::new()?;
        let device = open_device(id, direction, action)?;
        let client = device
            .get_iaudioclient()
            .map_err(|e| map_error(action, e))?;
        Ok(mix_format(&client, action)?.1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_the_errors_users_can_act_on() {
        let cases = [
            (0x8007_0490_u32, "not found"),
            (0x8889_0004, "unplugged"),
            (0x8889_000A, "exclusive mode"),
            (0x8889_0008, "format"),
            (0x8007_0005, "Privacy"),
            (0x8889_0010, "Windows Audio service"),
        ];
        for (code, expected) in cases {
            let text = describe_hresult(code as i32).unwrap();
            assert!(text.contains(expected), "{code:#x}: {text}");
        }
        assert_eq!(describe_hresult(0x8000_4005_u32 as i32), None);
    }

    #[test]
    fn audio_error_combines_action_and_reason() {
        let error = audio_error(
            "could not open the microphone",
            Some(0x8889_0004_u32 as i32),
            &"device invalidated",
        );
        assert_eq!(
            error.to_string(),
            "audio device error: could not open the microphone: the device was unplugged or disabled"
        );

        let error = audio_error("could not open the microphone", None, &"boom");
        assert!(error.to_string().contains("details are in the log"));
    }

    #[test]
    fn handle_reports_start_errors_and_stops_cleanly() {
        let result = spawn_stream::<(), _>("test stream", |_, started| {
            let _ = started.send(Err(AppError::Audio("nope".into())));
        });
        assert!(matches!(result, Err(AppError::Audio(message)) if message == "nope"));

        let (mut handle, value) = spawn_stream("test stream", |control, started| {
            let _ = started.send(Ok(42));
            while !control.should_stop() {
                std::thread::sleep(Duration::from_millis(1));
            }
        })
        .unwrap();
        assert_eq!(value, 42);
        assert!(handle.is_running());
        handle.stop();
        assert!(!handle.is_running());
        assert_eq!(handle.failure(), None);
    }

    #[test]
    fn handle_keeps_the_first_failure() {
        let (mut handle, ()) = spawn_stream("test stream", |control, started| {
            let _ = started.send(Ok(()));
            control.fail(&AppError::Audio("first".into()));
            control.fail(&AppError::Audio("second".into()));
        })
        .unwrap();
        handle.stop();
        assert_eq!(handle.failure(), Some("audio device error: first"));
    }
}
