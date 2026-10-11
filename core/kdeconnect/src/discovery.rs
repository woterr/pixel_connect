// Pixel Connect — KDE Connect protocol v8, LAN discovery (UDP broadcast + mDNS).
//
// Reproduces:
//   * kdeconnect-kde `LanLinkProvider::broadcastIdentityPacket()` /
//     `AvahiDiscovery` (UDP port 1716; mDNS type `_kdeconnect._udp` with TXT
//     records `id`, `name`, `type`, `protocol`)
//   * kdeconnect-android `LanLinkProvider` (`UDP_PORT = 1716`, `NsdManager`)
//
// Android additionally gates untrusted devices behind
// `TrustedNetworkHelper.isTrustedNetwork()`; we surface the network name so the
// application layer can apply the same policy.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Mutex, OnceLock};

use crate::deviceinfo::{discovery_packet_port, DeviceInfo, PROTOCOL_VERSION};

/// mDNS service type used by KDE Connect on both desktop and Android.
pub use crate::lan::MDNS_SERVICE_TYPE;

#[derive(Debug, Clone)]
pub struct DiscoveredPeer {
    pub device_id: String,
    pub name: String,
    pub device_type: Option<String>,
    pub protocol_version: i64,
    pub tcp_port: u16,
    pub address: IpAddr,
    pub source: &'static str,
    /// When this peer was last heard from, as a unix timestamp in seconds.
    ///
    /// Entries used to live forever, so a peer seen once - on a hotspot that is
    /// long gone, say - was dialled every refresh tick indefinitely. That is how
    /// the app accumulated several hundred half-open connections to an address
    /// that no longer existed.
    pub last_seen: u64,
}

struct Registry {
    peers: HashMap<String, DiscoveredPeer>,
    local_port: u16,
    network_name: String,
}

fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        Mutex::new(Registry {
            peers: HashMap::new(),
            local_port: crate::lan::MIN_TCP_PORT,
            network_name: String::new(),
        })
    })
}

/// Record the local TCP port so outbound connections can target the right port.
pub fn note_local_tcp_port(port: u16) {
    registry().lock().unwrap().local_port = port;
}

pub fn local_tcp_port() -> u16 {
    registry().lock().unwrap().local_port
}

/// Rank an interface for advertising.
///
/// Returns `None` for interfaces that cannot carry a peer connection, and a
/// higher score for better ones. Exposed so the choice can be tested without a
/// network, because getting it wrong is invisible until the user connects a
/// phone by hotspot and nothing appears.
pub fn score_interface(iface: &str, addr: Ipv4Addr, is_loopback: bool) -> Option<i32> {
    if is_loopback {
        return None;
    }
    // Point-to-point links (a hotspot client interface, a VPN) are the whole
    // point of this feature, so they must not be filtered out. They simply do
    // not get priority.
    let p2p = iface.starts_with("tailscale") || iface.starts_with("wg") || iface.starts_with("ppp");

    let base = if p2p { 10 } else { 50 };

    // An RFC1918 address is what a phone hotspot and a home router both hand
    // out, so it is a strong signal that this is the link the peer is on.
    let private = addr.is_private() || addr.is_link_local();

    Some(base + if private { 20 } else { 0 })
}

/// The local IPv4 address we advertise over mDNS.
///
/// This must be the address on the interface the phone is actually reachable
/// on. Returning "the first address the OS lists" is wrong whenever more than
/// one interface is up, which is the normal case: with a VPN or a virtual
/// machine active, that is frequently not the interface the phone is on, and
/// the phone then dials an address it cannot route to.
///
/// Falls back to the old behaviour (any non-loopback IPv4) rather than
/// returning loopback, so a machine with an unusual interface is still
/// advertised rather than silently becoming invisible.
pub fn primary_local_ipv4() -> Ipv4Addr {
    if let Ok(addrs) = get_if_addrs::get_if_addrs() {
        let mut best: Option<(i32, Ipv4Addr)> = None;
        for a in addrs {
            let IpAddr::V4(v4) = a.ip() else {
                continue;
            };
            let Some(score) = score_interface(&a.name, v4, a.is_loopback()) else {
                continue;
            };
            if best.is_none_or(|(b, _)| score > b) {
                best = Some((score, v4));
            }
        }
        if let Some((_, addr)) = best {
            return addr;
        }
    }
    Ipv4Addr::new(127, 0, 0, 1)
}

/// Best-effort local network name, used to warn the user when an untrusted
/// device appears on a public network.
pub fn local_network_name() -> String {
    registry().lock().unwrap().network_name.clone()
}

pub fn set_local_network_name(name: String) {
    registry().lock().unwrap().network_name = name;
}

fn upsert(peer: DiscoveredPeer) -> Option<DiscoveredPeer> {
    let mut reg = registry().lock().unwrap();
    reg.peers.insert(peer.device_id.clone(), peer.clone());
    Some(peer)
}

/// Handle a UDP identity broadcast.
pub fn note_udp_peer(from: IpAddr, packet: &crate::packet::NetworkPacket) {
    if !packet.is_identity() {
        return;
    }
    let Some(id) = packet.get_str("deviceId") else {
        return;
    };
    if !crate::deviceinfo::is_valid_device_id(&id) {
        return;
    }
    if Some(&from)
        == registry()
            .lock()
            .unwrap()
            .peers
            .get(&id)
            .map(|p| &p.address)
    {
        return;
    }
    let _ = upsert(DiscoveredPeer {
        device_id: id,
        name: packet.get_str("deviceName").unwrap_or_default(),
        device_type: packet.get_str("deviceType"),
        protocol_version: packet.body.get_i64("protocolVersion").unwrap_or(0),
        tcp_port: discovery_packet_port(packet).unwrap_or(crate::lan::MIN_TCP_PORT),
        address: from,
        source: "udp",
        last_seen: now_secs(),
    });
}

/// Handle a resolved mDNS service.
pub fn note_mdns_peer(
    address: Option<IpAddr>,
    port: u16,
    _instance: &str,
    txt: &HashMap<String, String>,
) {
    let Some(address) = address else { return };
    let Some(id) = txt.get("id") else { return };
    if !crate::deviceinfo::is_valid_device_id(id) {
        return;
    }
    let protocol = txt
        .get("protocol")
        .and_then(|p| p.parse::<i64>().ok())
        .unwrap_or(PROTOCOL_VERSION);
    let _ = upsert(DiscoveredPeer {
        device_id: id.clone(),
        name: txt.get("name").cloned().unwrap_or_default(),
        device_type: txt.get("type").cloned(),
        protocol_version: protocol,
        tcp_port: if port == 0 {
            crate::lan::MIN_TCP_PORT
        } else {
            port
        },
        address,
        source: "mdns",
        last_seen: now_secs(),
    });
}

/// Convert a stored peer into the full identity, filling in placeholders when
/// the cleartext packet did not carry them.
pub fn to_device_info(peer: &DiscoveredPeer) -> DeviceInfo {
    DeviceInfo {
        id: peer.device_id.clone(),
        name: crate::deviceinfo::sanitize_device_name(&peer.name),
        device_type: peer
            .device_type
            .as_deref()
            .and_then(crate::deviceinfo::DeviceType::parse)
            .unwrap_or(crate::deviceinfo::DeviceType::Desktop),
        protocol_version: peer.protocol_version,
        incoming_capabilities: Vec::new(),
        outgoing_capabilities: Vec::new(),
    }
}

/// How long a peer stays in the registry after it was last heard from.
///
/// The identity broadcast runs every 5 s, so this is generous - it only exists
/// to drop peers that have genuinely gone rather than to be responsive.
pub const PEER_TTL_SECS: u64 = 120;

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Peers heard from recently enough to be worth connecting to.
///
/// Pruning happens here rather than on a timer so there is no background task
/// to keep alive: a stale entry simply stops being returned, and it is dropped
/// from the map on the next read.
pub fn snapshot() -> Vec<DiscoveredPeer> {
    let mut reg = registry().lock().unwrap();
    let cutoff = now_secs().saturating_sub(PEER_TTL_SECS);
    reg.peers.retain(|_, p| p.last_seen >= cutoff);
    reg.peers.values().cloned().collect()
}

/// Forget peers that have gone quiet. Called on the refresh tick.
pub fn expire_stale() -> usize {
    let mut reg = registry().lock().unwrap();
    let cutoff = now_secs().saturating_sub(PEER_TTL_SECS);
    let before = reg.peers.len();
    reg.peers.retain(|_, p| p.last_seen >= cutoff);
    before - reg.peers.len()
}

pub fn target(device_id: &str) -> Option<DiscoveredPeer> {
    registry().lock().unwrap().peers.get(device_id).cloned()
}

pub fn forget(device_id: &str) {
    registry().lock().unwrap().peers.remove(device_id);
}

pub fn target_addr(peer: &DiscoveredPeer) -> SocketAddr {
    SocketAddr::new(peer.address, peer.tcp_port)
}

/// Addresses we should broadcast identity to.
/// Addresses to send the identity broadcast to.
///
/// `0.0.0.0` must **not** be in this list. On Linux, sending a datagram to the
/// unspecified address is delivered to the loopback interface only - verified
/// on this machine:
///
/// ```text
/// 0.0.0.0      -> received from 127.0.0.1
/// 10.141.5.255 -> received from 10.141.5.105
/// ```
///
/// So including it made every broadcast a no-op that the local socket looped
/// back to us. The phone never saw an identity packet, which is why KDE
/// Connect discovery failed while mDNS - a separate path - still found the
/// phone.
///
/// Each usable interface contributes its own directed broadcast address. An
/// unicast copy to the interface address is also sent, because some access
/// points drop broadcast frames but forward unicast, and a hotspot is more
/// likely to do that than a home router.
pub fn broadcast_addresses() -> Vec<Ipv4Addr> {
    let mut out: Vec<Ipv4Addr> = Vec::new();
    if let Ok(addrs) = get_if_addrs::get_if_addrs() {
        for a in addrs {
            if a.is_loopback() {
                continue;
            }
            let get_if_addrs::IfAddr::V4(v4) = a.addr else {
                continue;
            };
            // 255.255.255.255 reaches every network, including one attached to
            // this host, so it is sent once rather than per interface.
            out.push(Ipv4Addr::BROADCAST);
            // The directed broadcast for this subnet, e.g. 10.141.5.255. This is
            // what actually leaves the interface on most networks.
            if let Some(directed) = prefix_from_netmask(v4.netmask)
                .and_then(|prefix| directed_broadcast(v4.ip, prefix))
            {
                out.push(directed);
            }
            // Unicast to ourselves on that interface, which a hotspot will
            // forward even when it filters broadcast.
            out.push(v4.ip);
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// The directed broadcast address for an interface's subnet.
///
/// Returns `None` when the prefix cannot be determined or the address is not in
/// a usable subnet, rather than guessing: a wrong broadcast address is sent
/// to nowhere and looks identical to a silent network.
pub fn directed_broadcast(addr: Ipv4Addr, prefix: u8) -> Option<Ipv4Addr> {
    // A /0 would mean the entire internet, and a /32 is a single host with no
    // broadcast address - in that case the arithmetic yields the address
    // itself, which is a unicast, not a broadcast.
    if prefix == 0 || prefix >= 32 {
        return None;
    }
    let host_bits = 32 - prefix;
    let mask = u32::MAX << host_bits;
    let network = u32::from(addr) & mask;
    Some(Ipv4Addr::from(network | !mask))
}

/// Prefix length of `netmask`, or `None` when it is absent or nonsensical.
///
/// A hotspot hands out a /24 and a home router usually the same, but the mask
/// is read from the interface rather than assumed, because a wrong mask
/// produces a broadcast address that goes nowhere.
fn prefix_from_netmask(netmask: Ipv4Addr) -> Option<u8> {
    let bits = u32::from(netmask);
    // A valid mask is a run of ones followed by zeros, e.g. 0xFFFFFF00.
    let inverted = !bits;
    if inverted == u32::MAX {
        return None; // /0 would broadcast to the internet
    }
    if inverted & inverted.wrapping_add(1) != 0 {
        return None; // non-contiguous, i.e. not a real netmask
    }
    Some(bits.count_ones() as u8)
}

#[cfg(test)]
/// A peer that stops announcing must stop being dialled.
///
/// This was a real resource leak: the registry kept every peer it had ever
/// seen, and `refresh_devices` dials each one every tick. A phone that had
/// been on a hotspot and was no longer reachable stayed in the table, so the
/// app held hundreds of SYN-SENT sockets against an address that no longer
/// answered.
mod expiry_tests {
    use super::*;

    #[test]
    fn stale_entries_are_dropped_and_fresh_ones_kept() {
        let mut reg = registry().lock().unwrap();
        let now = now_secs();
        reg.peers.insert(
            "fresh".into(),
            DiscoveredPeer {
                device_id: "fresh".into(),
                name: "Fresh".into(),
                device_type: Some("phone".into()),
                protocol_version: 8,
                tcp_port: 1716,
                address: "10.0.0.1".parse().unwrap(),
                source: "udp",
                last_seen: now,
            },
        );
        reg.peers.insert(
            "stale".into(),
            DiscoveredPeer {
                device_id: "stale".into(),
                name: "Stale".into(),
                device_type: Some("phone".into()),
                protocol_version: 8,
                tcp_port: 1716,
                address: "192.168.29.8".parse().unwrap(),
                source: "udp",
                // Older than the TTL: this is a hotspot peer that went away.
                last_seen: now.saturating_sub(PEER_TTL_SECS + 1),
            },
        );
        drop(reg);

        let live = snapshot();
        let ids: Vec<&str> = live.iter().map(|p| p.device_id.as_str()).collect();
        assert!(ids.contains(&"fresh"), "a current peer must survive");
        assert!(
            !ids.contains(&"stale"),
            "a peer past its TTL must not be dialled again"
        );

        // The registry is process-wide, so leaving an entry behind would leak
        // into other tests that assert on it.
        registry().lock().unwrap().peers.remove("fresh");
    }

    #[test]
    fn the_ttl_outlasts_several_broadcast_intervals() {
        // The identity broadcast is every 5 s. A TTL shorter than a couple of
        // intervals would drop healthy peers between announcements.
        assert!(
            PEER_TTL_SECS >= 30,
            "a {PEER_TTL_SECS}s TTL is too tight for a 5s broadcast"
        );
    }
}

mod tests {
    use super::*;

    #[test]
    fn mdns_txt_records_match_upstream_layout() {
        // TXT key names come straight from avahidiscovery.cpp.
        let mut txt = HashMap::new();
        txt.insert("id".to_string(), "a".repeat(32));
        txt.insert("name".to_string(), "Pixel".to_string());
        txt.insert("type".to_string(), "phone".to_string());
        txt.insert("protocol".to_string(), "8".to_string());
        note_mdns_peer(
            Some("192.168.1.9".parse().unwrap()),
            1716,
            "x._kdeconnect._udp.local",
            &txt,
        );
        let peer = target(&"a".repeat(32)).unwrap();
        assert_eq!(peer.tcp_port, 1716);
        assert_eq!(peer.protocol_version, 8);
        assert_eq!(peer.source, "mdns");
        forget(&"a".repeat(32));
    }

    #[test]
    fn invalid_device_ids_are_ignored() {
        let mut np = crate::packet::NetworkPacket::new(crate::packet::PACKET_TYPE_IDENTITY);
        np.set("deviceId", "too-short");
        np.set("deviceName", "x");
        np.set("protocolVersion", 8);
        np.set("tcpPort", 1716);
        note_udp_peer("10.0.0.5".parse().unwrap(), &np);
        assert!(snapshot().is_empty());
    }

    // ---- interface selection ------------------------------------------------
    //
    // Choosing the wrong interface to advertise is silent: the app starts, the
    // port is bound on every interface, and the phone simply never appears.
    // These tests pin the decision that the hotspot case depends on.

    #[test]
    fn loopback_is_never_advertised() {
        assert_eq!(score_interface("lo", Ipv4Addr::new(127, 0, 0, 1), true), None);
    }

    /// The hotspot case: a phone hotspot hands out 192.168.x.x, which is also
    /// what a home router does, so a private address must win.
    #[test]
    fn a_private_address_beats_a_public_one() {
        let wifi = score_interface("wlo1", Ipv4Addr::new(192, 168, 43, 12), false).unwrap();
        let public = score_interface("wlo1", Ipv4Addr::new(203, 0, 113, 7), false).unwrap();
        assert!(wifi > public, "{wifi} should beat {public}");
    }

    /// With a VPN up, the tunnel's address must not be advertised, or the phone
    /// is told to dial an address on a network it is not part of.
    #[test]
    fn a_vpn_is_never_preferred_over_a_real_network() {
        let vpn = score_interface("tailscale0", Ipv4Addr::new(100, 114, 95, 4), false).unwrap();
        let wifi = score_interface("wlo1", Ipv4Addr::new(192, 168, 1, 20), false).unwrap();
        assert!(
            vpn < wifi,
            "a VPN interface must not outrank the local network"
        );
    }

    /// A point-to-point interface is still usable - it is just not first choice.
    #[test]
    fn point_to_point_interfaces_remain_usable() {
        let tun = score_interface("tailscale0", Ipv4Addr::new(100, 114, 95, 4), false);
        assert!(tun.is_some(), "a VPN must not be excluded, only deprioritised");
    }

    #[test]
    fn every_ordinary_interface_is_still_a_candidate() {
        for iface in ["wlo1", "wlan0", "eth0", "enp3s0"] {
            assert!(
                score_interface(iface, Ipv4Addr::new(10, 0, 0, 5), false).is_some(),
                "{iface} must remain advertisable"
            );
        }
    }

    // ---- broadcast addresses ------------------------------------------------

    /// The bug this pins: `0.0.0.0` was the first entry, and on Linux a
    /// datagram sent to it is delivered to loopback only. Every identity
    /// broadcast was therefore looped back to this machine and never reached
    /// the phone, so KDE Connect discovery silently did nothing.
    #[test]
    fn the_unspecified_address_is_never_broadcast_to() {
        let addrs = broadcast_addresses();
        assert!(
            !addrs.contains(&Ipv4Addr::UNSPECIFIED),
            "0.0.0.0 is delivered to 127.0.0.1 and reaches nothing"
        );
    }

    #[test]
    fn the_host_broadcast_is_included() {
        // 255.255.255.255 leaves every attached network, so it must be sent.
        assert!(broadcast_addresses().contains(&Ipv4Addr::BROADCAST));
    }

    #[test]
    fn loopback_is_never_a_destination() {
        let addrs = broadcast_addresses();
        assert!(
            !addrs.iter().any(|a| a.is_loopback()),
            "loopback must not be a broadcast target"
        );
    }

    #[test]
    fn directed_broadcasts_are_computed_from_the_netmask() {
        assert_eq!(
            directed_broadcast(Ipv4Addr::new(10, 141, 5, 105), 24),
            Some(Ipv4Addr::new(10, 141, 5, 255))
        );
        assert_eq!(
            directed_broadcast(Ipv4Addr::new(192, 168, 1, 20), 24),
            Some(Ipv4Addr::new(192, 168, 1, 255))
        );
        assert_eq!(
            directed_broadcast(Ipv4Addr::new(172, 20, 10, 3), 16),
            Some(Ipv4Addr::new(172, 20, 255, 255))
        );
        // A /32 is a single host and has no broadcast address.
        assert_eq!(directed_broadcast(Ipv4Addr::new(10, 0, 0, 5), 32), None);
        // /0 would mean the entire internet.
        assert_eq!(directed_broadcast(Ipv4Addr::new(10, 0, 0, 5), 0), None);
    }

    #[test]
    fn netmasks_are_converted_to_prefixes() {
        assert_eq!(prefix_from_netmask(Ipv4Addr::new(255, 255, 255, 0)), Some(24));
        assert_eq!(prefix_from_netmask(Ipv4Addr::new(255, 255, 0, 0)), Some(16));
        assert_eq!(prefix_from_netmask(Ipv4Addr::new(255, 255, 255, 252)), Some(30));
        // /0 must be rejected: it would broadcast to the internet.
        assert_eq!(prefix_from_netmask(Ipv4Addr::new(0, 0, 0, 0)), None);
        // Not a contiguous run of ones, so not a real netmask.
        assert_eq!(prefix_from_netmask(Ipv4Addr::new(255, 0, 255, 0)), None);
    }
}
