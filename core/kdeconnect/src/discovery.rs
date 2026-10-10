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

pub fn snapshot() -> Vec<DiscoveredPeer> {
    registry().lock().unwrap().peers.values().cloned().collect()
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
pub fn broadcast_addresses() -> Vec<Ipv4Addr> {
    let mut out = vec![Ipv4Addr::UNSPECIFIED];
    if let Ok(addrs) = get_if_addrs::get_if_addrs() {
        for a in addrs {
            if !a.is_loopback() {
                if let IpAddr::V4(v4) = a.ip() {
                    out.push(v4);
                }
            }
        }
    }
    out
}

#[cfg(test)]
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
}
