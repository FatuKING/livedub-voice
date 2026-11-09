//! Detection of known virtual audio cables.
//!
//! Windows cannot create a virtual microphone without a driver, so the outgoing direction writes
//! to the playback side of a virtual cable and the call/game app uses its recording side as mic.
//! LiveDub Voice does not ship a driver: it recognizes the common ones by endpoint name. This list
//! is the single place that knows those names (used by the device list and by onboarding).

/// A virtual audio cable product and the endpoint name fragments that identify it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VirtualCable {
    /// Product name shown to the user.
    pub product: &'static str,
    /// Lowercase fragments; an endpoint whose name contains any of them belongs to the product.
    pub name_patterns: &'static [&'static str],
}

/// Known virtual cables, most common first.
pub const KNOWN_VIRTUAL_CABLES: &[VirtualCable] = &[
    VirtualCable {
        product: "VoiceMeeter",
        // Checked before VB-CABLE: VoiceMeeter endpoints also mention "VB-Audio".
        name_patterns: &["voicemeeter"],
    },
    VirtualCable {
        product: "VB-CABLE",
        // "CABLE Input (VB-Audio Virtual Cable)", "CABLE-A Output (VB-Audio Cable A)",
        // "Hi-Fi Cable Input (VB-Audio Hi-Fi Cable)", …
        name_patterns: &["vb-audio", "cable input", "cable output"],
    },
    VirtualCable {
        product: "Virtual-Audio-Driver",
        name_patterns: &["virtual audio driver", "virtual mic driver"],
    },
    VirtualCable {
        product: "Virtual Audio Cable",
        // "Line 1 (Virtual Audio Cable)"
        name_patterns: &["virtual audio cable"],
    },
];

/// Returns the virtual cable product an endpoint belongs to, judging by its friendly name.
pub fn detect_virtual_cable(device_name: &str) -> Option<&'static VirtualCable> {
    let name = device_name.to_lowercase();
    KNOWN_VIRTUAL_CABLES.iter().find(|cable| {
        cable
            .name_patterns
            .iter()
            .any(|pattern| name.contains(pattern))
    })
}

/// Whether an endpoint looks like a known virtual cable.
pub fn is_virtual_cable(device_name: &str) -> bool {
    detect_virtual_cable(device_name).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn product(name: &str) -> Option<&'static str> {
        detect_virtual_cable(name).map(|cable| cable.product)
    }

    #[test]
    fn detects_vb_cable_endpoints() {
        assert_eq!(
            product("CABLE Input (VB-Audio Virtual Cable)"),
            Some("VB-CABLE")
        );
        assert_eq!(
            product("CABLE Output (VB-Audio Virtual Cable)"),
            Some("VB-CABLE")
        );
        assert_eq!(
            product("CABLE-A Input (VB-Audio Cable A)"),
            Some("VB-CABLE")
        );
        assert_eq!(
            product("Hi-Fi Cable Input (VB-Audio Hi-Fi Cable)"),
            Some("VB-CABLE")
        );
    }

    #[test]
    fn detects_voicemeeter_before_generic_vb_audio() {
        assert_eq!(
            product("VoiceMeeter Input (VB-Audio VoiceMeeter VAIO)"),
            Some("VoiceMeeter")
        );
        assert_eq!(
            product("Voicemeeter Out B1 (VB-Audio Voicemeeter VAIO)"),
            Some("VoiceMeeter")
        );
    }

    #[test]
    fn detects_other_known_cables() {
        assert_eq!(
            product("Speakers (Virtual Audio Driver by MTT)"),
            Some("Virtual-Audio-Driver")
        );
        assert_eq!(
            product("Line 1 (Virtual Audio Cable)"),
            Some("Virtual Audio Cable")
        );
    }

    #[test]
    fn ignores_regular_devices() {
        assert!(!is_virtual_cable("Speakers (Realtek(R) Audio)"));
        assert!(!is_virtual_cable("Microphone (USB Audio Device)"));
        assert!(!is_virtual_cable("Headphones (WH-1000XM4 Stereo)"));
        assert!(!is_virtual_cable(""));
    }

    #[test]
    fn patterns_are_lowercase() {
        for cable in KNOWN_VIRTUAL_CABLES {
            for pattern in cable.name_patterns {
                assert_eq!(*pattern, pattern.to_lowercase(), "{}", cable.product);
            }
        }
    }
}
