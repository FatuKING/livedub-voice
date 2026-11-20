//! Incoming direction: other apps' audio -> translator (to the user's language) -> headphones,
//! with the original audio ducked while it runs.
//!
//! The source is the settings' source app: one app's process tree, or the whole system minus
//! LiveDub Voice (process loopback, see [`crate::audio::loopback`]). Ducking lowers exactly the
//! sessions that are captured (see [`crate::audio::ducking`]): in whole-system mode that is every
//! app playing on any device, system sounds included, except LiveDub Voice itself.
//!
//! The engine is shared with the outgoing direction ([`super::translation`]); this module holds
//! what is specific to incoming: its configuration from the settings and the device check.

use super::reconnect::ReconnectPolicy;
use super::translation::{Direction, InputSource, PipelineConfig};
use crate::audio::{AudioDevice, DeviceKind};
use crate::error::{AppError, AppResult};
use crate::settings::IncomingSettings;

/// The incoming pipeline's configuration from the saved settings.
pub fn config(settings: &IncomingSettings) -> PipelineConfig {
    PipelineConfig {
        direction: Direction::Incoming,
        input: InputSource::Apps(settings.source_app.clone()),
        output_device_id: settings.output_device_id.clone(),
        target_language: settings.target_language,
        original_volume: Some(settings.original_volume),
        reconnect: ReconnectPolicy::default(),
    }
}

/// Checks the headphones against the active devices before starting: a chosen output must be
/// connected ([`AppError::DeviceMissing`]); none chosen means the system default.
pub fn check_devices(settings: &IncomingSettings, devices: &[AudioDevice]) -> AppResult<()> {
    match settings.output_device_id.as_deref() {
        Some(id)
            if !devices
                .iter()
                .any(|device| device.kind == DeviceKind::Output && device.id == id) =>
        {
            Err(AppError::DeviceMissing(
                "the headphones (incoming output) are not connected".into(),
            ))
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{Language, SourceApp};

    fn device(id: &str, kind: DeviceKind) -> AudioDevice {
        AudioDevice {
            id: id.into(),
            name: id.into(),
            kind,
            is_default: false,
            is_virtual_cable: false,
        }
    }

    fn headphones(id: Option<&str>) -> IncomingSettings {
        IncomingSettings {
            output_device_id: id.map(str::to_owned),
            ..IncomingSettings::default()
        }
    }

    #[test]
    fn check_devices_needs_connected_headphones_or_the_default() {
        let devices = [
            device("mic", DeviceKind::Input),
            device("headphones", DeviceKind::Output),
        ];

        assert!(check_devices(&headphones(None), &devices).is_ok());
        assert!(check_devices(&headphones(Some("headphones")), &devices).is_ok());
        assert!(matches!(
            check_devices(&headphones(Some("gone")), &devices),
            Err(AppError::DeviceMissing(message)) if message.contains("headphones")
        ));
        // An input id is not an output.
        assert!(check_devices(&headphones(Some("mic")), &devices).is_err());
    }

    #[test]
    fn config_captures_the_source_app_and_ducks_it() {
        let settings = IncomingSettings {
            target_language: Language::Spanish,
            source_app: SourceApp::Process {
                name: "Discord.exe".into(),
            },
            output_device_id: Some("headphones".into()),
            original_volume: 35,
            ..IncomingSettings::default()
        };

        assert_eq!(
            config(&settings),
            PipelineConfig {
                direction: Direction::Incoming,
                input: InputSource::Apps(SourceApp::Process {
                    name: "Discord.exe".into()
                }),
                output_device_id: Some("headphones".into()),
                target_language: Language::Spanish,
                original_volume: Some(35),
                reconnect: ReconnectPolicy::default(),
            }
        );
    }
}
