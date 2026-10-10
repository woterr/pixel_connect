// Pixel Connect — KDE Connect protocol v8, pairing state machine.
//
// Reproduces `PairingHandler` from kdeconnect-kde
// (core/backends/pairinghandler.cpp) and the matching logic in
// kdeconnect-android (PairingHandler.kt):
//
//   request pairing  -> {"pair": true,  "timestamp": <unix seconds>}
//   accept pairing   -> {"pair": true}
//   cancel / unpair  -> {"pair": false}
//   timeout          -> 30 s, CLOCK_TOLERANCE 1800 s
//
// Trust is established by pinning the peer's TLS certificate at the moment the
// user accepts; the human-verifiable 8-character verification key is derived
// from both public keys plus the pairing timestamp so both sides can confirm
// they are talking to the device they think they are.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::config::now_secs;
use crate::crypto::pairing_verification_key;
use crate::packet::{NetworkPacket, PacketBody, PACKET_TYPE_PAIR};

pub const PAIRING_TIMEOUT_SECS: u64 = 30;
/// Upstream `ALLOWED_TIMESTAMP_TIME_DIFFERENCE_SECONDS`.
pub const CLOCK_TOLERANCE_SECS: i64 = 1800;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum PairState {
    NotPaired,
    Requested,
    RequestedByPeer,
    Paired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairingAction {
    /// We asked the peer to pair; nothing to do until the peer answers.
    AwaitingPeer { timestamp: i64 },
    /// The peer asked to pair — ask the user, then accept or reject.
    IncomingRequest {
        timestamp: i64,
        verification_key: String,
    },
    /// The peer accepted our request.
    PeerAccepted,
    /// The user (or peer) cancelled / rejected pairing.
    Cancelled,
    /// The peer's timestamp was outside the allowed window.
    StaleTimestamp,
    /// Nothing happened within `PAIRING_TIMEOUT_SECS`.
    TimedOut,
}

pub struct PairingHandler {
    state: PairState,
    timestamp: i64,
}

impl Default for PairingHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl PairingHandler {
    pub fn new() -> Self {
        Self {
            state: PairState::NotPaired,
            timestamp: 0,
        }
    }

    pub fn state(&self) -> PairState {
        self.state
    }

    pub fn set_state(&mut self, state: PairState) {
        self.state = state;
    }

    pub fn timestamp(&self) -> i64 {
        self.timestamp
    }

    /// Build the "please pair with me" packet.
    pub fn request_pairing_packet(&mut self) -> NetworkPacket {
        self.timestamp = now_secs();
        self.state = PairState::Requested;
        let mut body = PacketBody::new();
        body.set("timestamp", self.timestamp);
        body.set("pair", true);
        NetworkPacket::with_body(PACKET_TYPE_PAIR, body)
    }

    pub fn accept_packet(&self) -> NetworkPacket {
        let mut body = PacketBody::new();
        body.set("pair", true);
        NetworkPacket::with_body(PACKET_TYPE_PAIR, body)
    }

    pub fn cancel_packet(&self) -> NetworkPacket {
        let mut body = PacketBody::new();
        body.set("pair", false);
        NetworkPacket::with_body(PACKET_TYPE_PAIR, body)
    }

    pub fn cancel(&mut self) {
        self.state = PairState::NotPaired;
        self.timestamp = 0;
    }

    pub fn paired(&mut self) {
        self.state = PairState::Paired;
    }

    /// Handle an incoming packet.
    ///
    /// `trusted` says whether the device is already in the trust store;
    /// `our_public_key_der` is our DER SubjectPublicKeyInfo, needed to build the
    /// verification key for an incoming request.
    pub fn handle_packet(
        &mut self,
        packet: &NetworkPacket,
        trusted: bool,
        our_public_key_der: &[u8],
        their_public_key_der: &[u8],
    ) -> PairingAction {
        if packet.packet_type != PACKET_TYPE_PAIR {
            return PairingAction::Cancelled;
        }

        let pair = packet.body.get_bool("pair").unwrap_or(false);
        let timestamp = packet.body.get_i64("timestamp");

        if !pair {
            // Cancel / unpair / reject, from either side.
            self.cancel();
            return PairingAction::Cancelled;
        }

        match self.state {
            PairState::Requested => {
                // A packet without a timestamp is the peer accepting our
                // request; with a timestamp it is a fresh request from the peer.
                match timestamp {
                    None => {
                        self.state = PairState::Paired;
                        PairingAction::PeerAccepted
                    }
                    Some(ts) => {
                        if self.timestamp != 0 && (ts - self.timestamp).abs() > CLOCK_TOLERANCE_SECS
                        {
                            return PairingAction::StaleTimestamp;
                        }
                        self.state = PairState::RequestedByPeer;
                        self.timestamp = ts;
                        PairingAction::IncomingRequest {
                            timestamp: ts,
                            verification_key: pairing_verification_key(
                                our_public_key_der,
                                their_public_key_der,
                                ts,
                            ),
                        }
                    }
                }
            }
            PairState::RequestedByPeer => {
                // Already showing a prompt; a timestamped packet means the peer
                // re-requested (upstream drops us back to NotPaired first).
                if timestamp.is_some() {
                    self.state = PairState::NotPaired;
                    return self.handle_packet(packet, trusted, our_public_key_der, their_public_key_der);
                }
                PairingAction::PeerAccepted
            }
            PairState::Paired => {
                if timestamp.is_none() {
                    return PairingAction::Cancelled;
                }
                let ts = timestamp.unwrap_or_default();
                if (ts - now_secs()).abs() > CLOCK_TOLERANCE_SECS {
                    return PairingAction::StaleTimestamp;
                }
                self.timestamp = ts;
                self.state = PairState::RequestedByPeer;
                PairingAction::IncomingRequest {
                    timestamp: ts,
                    verification_key: pairing_verification_key(
                        our_public_key_der,
                        their_public_key_der,
                        ts,
                    ),
                }
            }
            PairState::NotPaired => {
                if trusted {
                    // Already trusted but we are not tracking a live pairing
                    // flow; nothing to do.
                    return PairingAction::Cancelled;
                }
                let ts = timestamp.unwrap_or_else(now_secs);
                if timestamp.is_some() && (ts - now_secs()).abs() > CLOCK_TOLERANCE_SECS {
                    return PairingAction::StaleTimestamp;
                }
                self.state = PairState::RequestedByPeer;
                self.timestamp = ts;
                PairingAction::IncomingRequest {
                    timestamp: ts,
                    verification_key: pairing_verification_key(
                        our_public_key_der,
                        their_public_key_der,
                        ts,
                    ),
                }
            }
        }
    }

    pub fn has_timed_out(&self, started_at: std::time::Instant) -> bool {
        matches!(self.state, PairState::Requested | PairState::RequestedByPeer)
            && started_at.elapsed() > std::time::Duration::from_secs(PAIRING_TIMEOUT_SECS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUR: &[u8] = b"our-public-key";
    const THEIR: &[u8] = b"their-public-key";

    fn accept_packet() -> NetworkPacket {
        let mut body = PacketBody::new();
        body.set("pair", true);
        NetworkPacket::with_body(PACKET_TYPE_PAIR, body)
    }

    fn request_packet(ts: i64) -> NetworkPacket {
        let mut body = PacketBody::new();
        body.set("pair", true);
        body.set("timestamp", ts);
        NetworkPacket::with_body(PACKET_TYPE_PAIR, body)
    }

    fn cancel_packet() -> NetworkPacket {
        let mut body = PacketBody::new();
        body.set("pair", false);
        NetworkPacket::with_body(PACKET_TYPE_PAIR, body)
    }

    #[test]
    fn incoming_request_carries_verification_key() {
        let mut h = PairingHandler::new();
        let now = now_secs();
        let action = h.handle_packet(&request_packet(now), false, OUR, THEIR);
        match action {
            PairingAction::IncomingRequest { verification_key, .. } => {
                assert_eq!(verification_key.len(), 8);
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(h.state(), PairState::RequestedByPeer);
    }

    #[test]
    fn peer_acceptance_of_our_request_marks_paired() {
        let mut h = PairingHandler::new();
        h.request_pairing_packet();
        assert_eq!(h.state(), PairState::Requested);
        assert_eq!(
            h.handle_packet(&accept_packet(), false, OUR, THEIR),
            PairingAction::PeerAccepted
        );
        assert_eq!(h.state(), PairState::Paired);
    }

    #[test]
    fn stale_timestamps_are_rejected() {
        let mut h = PairingHandler::new();
        let old = now_secs() - CLOCK_TOLERANCE_SECS - 60;
        assert_eq!(
            h.handle_packet(&request_packet(old), false, OUR, THEIR),
            PairingAction::StaleTimestamp
        );
        assert_eq!(h.state(), PairState::NotPaired);
    }

    #[test]
    fn unpair_from_peer_resets_state() {
        let mut h = PairingHandler::new();
        h.paired();
        assert_eq!(h.state(), PairState::Paired);
        assert_eq!(
            h.handle_packet(&cancel_packet(), true, OUR, THEIR),
            PairingAction::Cancelled
        );
        assert_eq!(h.state(), PairState::NotPaired);
    }

    #[test]
    fn timeout_only_applies_while_pairing() {
        let mut h = PairingHandler::new();
        let start = std::time::Instant::now();
        assert!(!h.has_timed_out(start));
        h.request_pairing_packet();
        assert!(!h.has_timed_out(start));
        h.paired();
        assert!(!h.has_timed_out(start + std::time::Duration::from_secs(120)));
    }
}