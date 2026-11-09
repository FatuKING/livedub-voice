//! Audio device listing (active WASAPI render and capture endpoints).
//!
//! [`list_devices`] talks to Windows; turning the raw endpoints into the sorted list the panel
//! shows ([`build_device_list`]) is pure so it can be unit tested.

use std::cmp::Ordering;

use serde::Serialize;

use super::virtual_cable::is_virtual_cable;
use crate::error::AppResult;

/// Whether a device records (microphone, cable output) or plays (speakers, cable input).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DeviceKind {
    /// A capture endpoint.
    Input,
    /// A render endpoint.
    Output,
}

/// An active audio endpoint, as shown in the panel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDevice {
    /// Stable WASAPI endpoint id (`{0.0.0.00000000}.{guid}`); this is what settings store.
    pub id: String,
    /// Friendly name, for example "Speakers (Realtek(R) Audio)".
    pub name: String,
    pub kind: DeviceKind,
    /// Whether this is the system default device of its kind (console role).
    pub is_default: bool,
    /// Whether the name matches a known virtual audio cable.
    pub is_virtual_cable: bool,
}

/// An endpoint as read from Windows, before flags and sorting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawEndpoint {
    pub id: String,
    pub name: String,
    pub kind: DeviceKind,
}

/// Builds the device list: sets the default and virtual cable flags and sorts it.
///
/// Order: inputs, then outputs; within a kind the default device first, then by name
/// (case-insensitive). Endpoints with the same id are kept once.
pub fn build_device_list(
    endpoints: Vec<RawEndpoint>,
    default_input_id: Option<&str>,
    default_output_id: Option<&str>,
) -> Vec<AudioDevice> {
    let mut devices: Vec<AudioDevice> = endpoints
        .into_iter()
        .map(|endpoint| {
            let default_id = match endpoint.kind {
                DeviceKind::Input => default_input_id,
                DeviceKind::Output => default_output_id,
            };
            AudioDevice {
                is_default: default_id == Some(endpoint.id.as_str()),
                is_virtual_cable: is_virtual_cable(&endpoint.name),
                id: endpoint.id,
                name: endpoint.name,
                kind: endpoint.kind,
            }
        })
        .collect();
    devices.sort_by(compare_devices);
    devices.dedup_by(|a, b| a.id == b.id);
    devices
}

fn compare_devices(a: &AudioDevice, b: &AudioDevice) -> Ordering {
    a.kind
        .cmp(&b.kind)
        .then_with(|| b.is_default.cmp(&a.is_default))
        .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        .then_with(|| a.id.cmp(&b.id))
}

/// Lists the active input and output devices.
///
/// Blocking: call it from a blocking thread (for example `tauri::async_runtime::spawn_blocking`).
/// It initializes COM on the calling thread for the duration of the call.
pub fn list_devices() -> AppResult<Vec<AudioDevice>> {
    let (endpoints, default_input, default_output) = windows_impl::enumerate()?;
    Ok(build_device_list(
        endpoints,
        default_input.as_deref(),
        default_output.as_deref(),
    ))
}

#[cfg(windows)]
mod windows_impl {
    use wasapi::{DeviceEnumerator, Direction};

    use super::{DeviceKind, RawEndpoint};
    use crate::error::{AppError, AppResult};

    /// `RPC_E_CHANGED_MODE`: COM is already initialized on this thread with another apartment
    /// model. COM is still usable then, it just must not be uninitialized by us.
    const RPC_E_CHANGED_MODE: i32 = 0x8001_0106_u32 as i32;

    /// Initializes COM (multithreaded apartment) on the current thread and uninitializes it on
    /// drop, but only if this guard's call succeeded.
    struct ComGuard {
        owns_init: bool,
    }

    impl ComGuard {
        fn new() -> AppResult<Self> {
            let hr = wasapi::initialize_mta();
            if hr.is_ok() {
                Ok(Self { owns_init: true })
            } else if hr.0 == RPC_E_CHANGED_MODE {
                Ok(Self { owns_init: false })
            } else {
                tracing::error!(hresult = hr.0, "COM initialization failed");
                Err(audio_error())
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

    fn audio_error() -> AppError {
        AppError::Audio("could not list audio devices".into())
    }

    type Enumeration = (Vec<RawEndpoint>, Option<String>, Option<String>);

    pub(super) fn enumerate() -> AppResult<Enumeration> {
        let _com = ComGuard::new()?;
        // Every COM object is created and dropped inside `enumerate_with_com`, before `_com`
        // uninitializes COM.
        enumerate_with_com().map_err(|error| {
            tracing::error!(%error, "audio device enumeration failed");
            audio_error()
        })
    }

    fn enumerate_with_com() -> Result<Enumeration, wasapi::WasapiError> {
        let enumerator = DeviceEnumerator::new()?;
        let mut endpoints = Vec::new();
        endpoints.extend(endpoints_of(&enumerator, Direction::Capture)?);
        endpoints.extend(endpoints_of(&enumerator, Direction::Render)?);
        let default_input = default_id(&enumerator, Direction::Capture);
        let default_output = default_id(&enumerator, Direction::Render);
        Ok((endpoints, default_input, default_output))
    }

    fn endpoints_of(
        enumerator: &DeviceEnumerator,
        direction: Direction,
    ) -> Result<Vec<RawEndpoint>, wasapi::WasapiError> {
        let kind = match direction {
            Direction::Capture => DeviceKind::Input,
            Direction::Render => DeviceKind::Output,
        };
        let collection = enumerator.get_device_collection(direction)?;
        let count = collection.get_nbr_devices()?;
        let mut endpoints = Vec::with_capacity(count as usize);
        for index in 0..count {
            // A device can disappear between the count and the read: skip it instead of failing
            // the whole list.
            let read = collection
                .get_device_at_index(index)
                .and_then(|device| Ok((device.get_id()?, device.get_friendlyname()?)));
            match read {
                Ok((id, name)) => endpoints.push(RawEndpoint { id, name, kind }),
                Err(error) => tracing::warn!(%error, index, ?kind, "skipping audio endpoint"),
            }
        }
        Ok(endpoints)
    }

    /// Id of the default device; `None` when there is none (for example no microphone at all).
    fn default_id(enumerator: &DeviceEnumerator, direction: Direction) -> Option<String> {
        enumerator
            .get_default_device(direction)
            .and_then(|device| device.get_id())
            .ok()
    }
}

#[cfg(not(windows))]
mod windows_impl {
    use super::RawEndpoint;
    use crate::error::AppResult;

    /// LiveDub Voice only supports Windows; elsewhere there are no devices to list.
    pub(super) fn enumerate() -> AppResult<(Vec<RawEndpoint>, Option<String>, Option<String>)> {
        Ok((Vec::new(), None, None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn raw(id: &str, name: &str, kind: DeviceKind) -> RawEndpoint {
        RawEndpoint {
            id: id.into(),
            name: name.into(),
            kind,
        }
    }

    #[test]
    fn sets_default_and_virtual_cable_flags() {
        let devices = build_device_list(
            vec![
                raw("spk", "Speakers (Realtek(R) Audio)", DeviceKind::Output),
                raw(
                    "cable-in",
                    "CABLE Input (VB-Audio Virtual Cable)",
                    DeviceKind::Output,
                ),
                raw("mic", "Microphone (USB)", DeviceKind::Input),
            ],
            Some("mic"),
            Some("spk"),
        );

        let flags: Vec<_> = devices
            .iter()
            .map(|d| (d.id.as_str(), d.is_default, d.is_virtual_cable))
            .collect();
        assert_eq!(
            flags,
            vec![
                ("mic", true, false),
                ("spk", true, false),
                ("cable-in", false, true),
            ]
        );
    }

    #[test]
    fn default_ids_only_match_their_own_kind() {
        // The same id as default output must not mark an input as default.
        let devices = build_device_list(
            vec![raw("same", "Thing", DeviceKind::Input)],
            None,
            Some("same"),
        );

        assert!(!devices[0].is_default);
    }

    #[test]
    fn sorts_inputs_first_then_default_then_name() {
        let devices = build_device_list(
            vec![
                raw("o-b", "beta speakers", DeviceKind::Output),
                raw("o-a", "Alpha speakers", DeviceKind::Output),
                raw("o-z", "Zulu speakers", DeviceKind::Output),
                raw("i-b", "Beta mic", DeviceKind::Input),
                raw("i-a", "alpha mic", DeviceKind::Input),
            ],
            None,
            Some("o-z"),
        );

        let ids: Vec<_> = devices.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(ids, vec!["i-a", "i-b", "o-z", "o-a", "o-b"]);
    }

    #[test]
    fn keeps_duplicate_ids_once() {
        let devices = build_device_list(
            vec![
                raw("x", "Speakers", DeviceKind::Output),
                raw("x", "Speakers", DeviceKind::Output),
            ],
            None,
            None,
        );

        assert_eq!(devices.len(), 1);
    }

    #[test]
    fn serializes_to_the_panel_shape() {
        let device = AudioDevice {
            id: "{0.0.0.00000000}.{abc}".into(),
            name: "CABLE Input (VB-Audio Virtual Cable)".into(),
            kind: DeviceKind::Output,
            is_default: false,
            is_virtual_cable: true,
        };

        assert_eq!(
            serde_json::to_value(&device).unwrap(),
            json!({
                "id": "{0.0.0.00000000}.{abc}",
                "name": "CABLE Input (VB-Audio Virtual Cable)",
                "kind": "output",
                "isDefault": false,
                "isVirtualCable": true
            })
        );
    }

    /// Lists the real devices of this machine. Run with
    /// `cargo test --manifest-path src-tauri/Cargo.toml list_real_devices -- --ignored --nocapture`.
    #[test]
    #[ignore = "needs real audio hardware"]
    fn list_real_devices() {
        let devices = list_devices().unwrap();
        for device in &devices {
            println!(
                "{:?}\t{}{}\t{}",
                device.kind,
                device.name,
                if device.is_default { " [default]" } else { "" },
                if device.is_virtual_cable {
                    "[virtual cable]"
                } else {
                    ""
                },
            );
        }
        println!("{} devices", devices.len());
    }
}
