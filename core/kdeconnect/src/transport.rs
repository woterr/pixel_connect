// Pixel Connect — transport registry.
//
// A transport is a way to reach a peer. LAN is implemented; Bluetooth and
// Wi-Fi Direct are declared so that the rest of the application has a single,
// honest place to ask "how could this device be reached", rather than each screen
// inventing its own list.
//
// The registry distinguishes three states deliberately, because conflating them
// is what makes UIs lie:
//
//   * `Available`  — implemented and compiled in
//   * `Unverified` — implemented but not yet exercised against a real peer
//   * `Unavailable`— not implemented, with a recorded reason
//
// Nothing is ever reported as available that is not. See
// `docs/architecture/v2-transport-findings.md` for how each state was
// determined on real hardware.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The transports Pixel Connect knows about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Transport {
    /// TCP + TLS over the local network, KDE Connect protocol v8.
    Lan,
    /// Bluetooth RFCOMM carrying the KDE Connect multiplexing protocol.
    Bluetooth,
    /// Wi-Fi Direct, with KDE Connect carried over the resulting IP link.
    WifiDirect,
    /// No live transport.
    None,
}

impl Transport {
    pub fn as_str(&self) -> &'static str {
        match self {
            Transport::Lan => "LAN",
            Transport::Bluetooth => "Bluetooth",
            Transport::WifiDirect => "Wi-Fi Direct",
            Transport::None => "None",
        }
    }

    /// Lower-case identifier used on the wire in `payloadTransferInfo`.
    pub fn as_protocol(&self) -> &'static str {
        match self {
            Transport::Lan => "lan",
            Transport::Bluetooth => "bluetooth",
            Transport::WifiDirect => "wifidirect",
            Transport::None => "none",
        }
    }
}

/// Implementation status of a transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum TransportState {
    /// Implemented, compiled in, and exercised against a real peer.
    Available,
    /// Implemented but not yet verified against a real peer.
    Unverified,
    /// Not implemented in this build.
    Unavailable,
}

impl TransportState {
    pub fn as_str(&self) -> &'static str {
        match self {
            TransportState::Available => "available",
            TransportState::Unverified => "unverified",
            TransportState::Unavailable => "unavailable",
        }
    }
}

/// What the user should be told about a transport.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TransportStatus {
    pub transport: Transport,
    pub name: &'static str,
    pub state: TransportState,
    /// One sentence, in plain language, saying what is and is not possible.
    pub summary: &'static str,
    /// The concrete blocker, when `state` is not `Available`.
    pub blocker: Option<&'static str>,
    /// True when the transport can carry clipboard and file traffic today.
    pub carries_data: bool,
}

/// Report on every transport.
///
/// The three non-LAN entries are deliberately reported as unavailable with a
/// measured reason rather than shown as a control that would do nothing.
pub fn registry() -> Vec<TransportStatus> {
    vec![
        TransportStatus {
            transport: Transport::Lan,
            name: "Local network",
            state: TransportState::Available,
            summary: "Discovery, pairing, clipboard and file sharing over TCP and TLS \
                      on the local network.",
            blocker: None,
            carries_data: true,
        },
        TransportStatus {
            transport: Transport::Bluetooth,
            name: "Bluetooth",
            state: TransportState::Unavailable,
            summary: "Not implemented in this build.",
            blocker: Some(
                "KDE Connect's Bluetooth backend needs an RFCOMM multiplexer that is \
                 disabled upstream (KDE bugs 520578 and 513536), and the Android app \
                 requires operating-system Bluetooth pairing before it will connect \
                 and keeps its own Bluetooth transport off by default. No Pixel was \
                 attached during development, so the multiplexer could not be \
                 exercised against a real peer. See docs/architecture/v2-transport-findings.md.",
            ),
            carries_data: false,
        },
        TransportStatus {
            transport: Transport::WifiDirect,
            name: "Wi-Fi Direct",
            state: TransportState::Unavailable,
            summary: "Not implemented in this build.",
            blocker: Some(
                "The wireless adapter supports P2P-client, P2P-GO and P2P-device, but \
                 NetworkManager 1.56 exposes no P2P API (no D-Bus methods, no nmcli verb, \
                 no wifi-p2p connection type), so the only route is driving wpa_supplicant \
                 directly, which needs root and would conflict with NetworkManager's \
                 ownership of the interface. KDE Connect itself has no Wi-Fi Direct \
                 support at all. See docs/architecture/v2-transport-findings.md.",
            ),
            carries_data: false,
        },
    ]
}

/// Just the transports that can carry traffic right now.
pub fn available() -> Vec<Transport> {
    registry()
        .into_iter()
        .filter(|t| t.state == TransportState::Available && t.carries_data)
        .map(|t| t.transport)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_verified_transports_are_reported_available() {
        // This is the invariant that stops the UI from lying: nothing may claim
        // to be available unless it has actually been exercised.
        let lan = registry()
            .into_iter()
            .find(|t| t.transport == Transport::Lan)
            .expect("LAN is always registered");
        assert_eq!(lan.state, TransportState::Available);
        assert!(lan.carries_data);
        assert_eq!(available(), vec![Transport::Lan]);
    }

    #[test]
    fn every_unavailable_transport_explains_itself() {
        for t in registry() {
            if t.state != TransportState::Available {
                assert!(
                    t.blocker.is_some_and(|b| b.len() > 40),
                    "{} has no real explanation",
                    t.name
                );
                assert!(
                    !t.carries_data,
                    "{} is unavailable but claims to carry data",
                    t.name
                );
            }
        }
    }

    #[test]
    fn transport_names_are_stable_for_the_ui() {
        assert_eq!(Transport::Lan.as_str(), "LAN");
        assert_eq!(Transport::Lan.as_protocol(), "lan");
        assert_eq!(Transport::None.as_str(), "None");
        // The wire identifier is what a peer would put in payloadTransferInfo.
        assert_eq!(Transport::WifiDirect.as_protocol(), "wifidirect");
    }

    #[test]
    fn the_registry_serialises_for_the_frontend() {
        let json = serde_json::to_value(registry()).unwrap();
        let list = json.as_array().expect("an array of transports");
        assert_eq!(list.len(), 3);
        assert!(list[0].get("state").is_some());
        assert!(list[0].get("carriesData").is_some());
    }
}