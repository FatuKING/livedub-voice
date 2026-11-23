//! Outgoing direction: the user's microphone -> translator (to the other side's language) ->
//! virtual cable, which the call or game app uses as its microphone.
//!
//! The engine is shared with the incoming direction ([`super::translation`]); this module holds
//! what is specific to outgoing: its configuration from the settings and the device check.

use super::reconnect::ReconnectPolicy;
use super::translation::{Direction, InputSource, PipelineConfig};
use super::voice::{TalkKey, VoiceConfig};
use crate::audio::{AudioDevice, DeviceKind};
use crate::error::{AppError, AppResult};
use crate::settings::{OutgoingSettings, TalkMode, VoiceActivitySettings};

/// The outgoing pipeline's configuration from the saved settings (its own, and the voice
/// detection shared by both directions). In push-to-talk mode the gate follows `talk_key`.
pub fn config(
    settings: &OutgoingSettings,
    voice: &VoiceActivitySettings,
    talk_key: &TalkKey,
) -> PipelineConfig {
    let push_to_talk = settings.talk_mode == TalkMode::PushToTalk;
    PipelineConfig {
        direction: Direction::Outgoing,
        input: InputSource::Device(settings.input_device_id.clone()),
        output_device_id: settings.output_device_id.clone(),
        target_language: settings.target_language,
        original_volume: None,
        reconnect: ReconnectPolicy::default(),
        voice: VoiceConfig {
            push_to_talk,
            ..VoiceConfig::from_settings(voice)
        },
        talk_key: push_to_talk.then(|| talk_key.clone()),
    }
}

/// Checks the outgoing devices against the active ones before starting.
///
/// Fails when no output is chosen ([`AppError::NoOutputDevice`]) or a chosen device is not
/// connected ([`AppError::DeviceMissing`]). Returns whether the output is a known virtual cable:
/// any other output still works (the user may know better), so it only deserves a warning.
pub fn check_devices(settings: &OutgoingSettings, devices: &[AudioDevice]) -> AppResult<bool> {
    let output_id = settings
        .output_device_id
        .as_deref()
        .ok_or(AppError::NoOutputDevice)?;
    let output = devices
        .iter()
        .find(|device| device.kind == DeviceKind::Output && device.id == output_id)
        .ok_or_else(|| {
            AppError::DeviceMissing("the output device (virtual cable) is not connected".into())
        })?;
    if let Some(input_id) = settings.input_device_id.as_deref()
        && !devices
            .iter()
            .any(|device| device.kind == DeviceKind::Input && device.id == input_id)
    {
        return Err(AppError::DeviceMissing(
            "the microphone is not connected".into(),
        ));
    }
    Ok(output.is_virtual_cable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Language;

    fn device(id: &str, kind: DeviceKind, is_virtual_cable: bool) -> AudioDevice {
        AudioDevice {
            id: id.into(),
            name: id.into(),
            kind,
            is_default: false,
            is_virtual_cable,
        }
    }

    fn outgoing(input: Option<&str>, output: Option<&str>) -> OutgoingSettings {
        OutgoingSettings {
            input_device_id: input.map(str::to_owned),
            output_device_id: output.map(str::to_owned),
            ..OutgoingSettings::default()
        }
    }

    #[test]
    fn check_devices_needs_a_connected_output_and_microphone() {
        let devices = [
            device("mic", DeviceKind::Input, false),
            device("cable", DeviceKind::Output, true),
            device("speakers", DeviceKind::Output, false),
        ];

        assert!(matches!(
            check_devices(&outgoing(None, None), &devices),
            Err(AppError::NoOutputDevice)
        ));
        assert!(matches!(
            check_devices(&outgoing(None, Some("gone")), &devices),
            Err(AppError::DeviceMissing(message)) if message.contains("output")
        ));
        // An input id is not an output.
        assert!(check_devices(&outgoing(None, Some("mic")), &devices).is_err());
        assert!(matches!(
            check_devices(&outgoing(Some("gone"), Some("cable")), &devices),
            Err(AppError::DeviceMissing(message)) if message.contains("microphone")
        ));
        assert!(check_devices(&outgoing(Some("mic"), Some("cable")), &devices).unwrap());
        assert!(check_devices(&outgoing(None, Some("cable")), &devices).unwrap());
        // Not a known cable: allowed, but reported.
        assert!(!check_devices(&outgoing(None, Some("speakers")), &devices).unwrap());
    }

    #[test]
    fn config_captures_the_microphone_without_ducking() {
        let settings = OutgoingSettings {
            target_language: Language::Japanese,
            ..outgoing(Some("mic"), Some("cable"))
        };

        assert_eq!(
            config(
                &settings,
                &VoiceActivitySettings {
                    pause_when_silent: true,
                    auto_off_minutes: None,
                },
                &TalkKey::default(),
            ),
            PipelineConfig {
                direction: Direction::Outgoing,
                input: InputSource::Device(Some("mic".into())),
                output_device_id: Some("cable".into()),
                target_language: Language::Japanese,
                original_volume: None,
                reconnect: ReconnectPolicy::default(),
                voice: VoiceConfig::default(),
                talk_key: None,
            }
        );
    }
}
