# Repository audit

This document records what was actually found in the source repositories before
any Pixel Connect code was written, and which parts of each project were reused,
adapted, ported or deliberately not used.

Everything below was read from the upstream sources at the revisions listed in
§0. Claims about behaviour are marked **verified** when the statement was
checked against code and a test, and **not verified** when it could not be
checked in this environment (for example because no Pixel phone was attached).

---

## 0. Sources and revisions

| Repository | URL | Revision inspected |
|---|---|---|
| rquickshare | https://github.com/Martichou/rquickshare | `378d8ae` (master, "feat(BREAKING): remove legacy variant"), tag `v0.11.5` |
| kdeconnect-kde | https://github.com/KDE/kdeconnect-kde | `44a9597` (master) |
| kdeconnect-android | https://github.com/KDE/kdeconnect-android | `44a9597` (master, `applicationId = org.kde.kdeconnect_tp`) |
| KDE Connect docs | https://github.com/KDE/kdeconnect-meta | referenced, but protocol claims were checked against code instead |

The brief suggested the KDE Connect documentation would be sufficient. It is
not: several widely repeated facts about KDE Connect are **wrong for protocol
version 8** (see §3.5). Everything below was therefore verified against the
actual desktop and Android implementations.

---

## 1. rquickshare

### 1.1 Project structure

```
rquickshare/
├── core_lib/          # `rqs_lib` — the protocol, a plain Rust library
│   ├── build.rs       # compiles the .proto files with prost-build
│   ├── bindings/      # ts-rs generated TypeScript models
│   └── src/
│       ├── lib.rs     # `RQS` service facade (run / discovery / stop)
│       ├── manager.rs # `TcpServer`, `SendInfo`
│       ├── channel.rs # `ChannelMessage` used to talk to the UI
│       ├── utils.rs   # mDNS naming, UKEY2 helpers, the 4-digit PIN
│       └── hdl/
│           ├── inbound.rs   (1348 lines) — receiving side
│           ├── outbound.rs  (1255 lines) — sending side
│           ├── mdns.rs      — mDNS advertisement
│           ├── mdns_discovery.rs — mDNS browsing
│           ├── ble.rs       — BLE listener
│           ├── blea.rs      — BLE advertiser (BlueZ / bluer)
│           └── info.rs
└── app/main/
    ├── src/           # Vue 3 + Tailwind front end
    └── src-tauri/     # Tauri 2 application
```

Total protocol implementation: ~3900 lines of Rust. **Verified** by inspection.

### 1.2 Technology

| Concern | Answer |
|---|---|
| Tauri version | 2.2, plugins: autostart, process, notification, single-instance, store, dialog, shell, clipboard-manager |
| Rust edition | 2021 |
| Front-end framework | Vue 3.5 with `unplugin-auto-import` |
| Styling | Tailwind CSS 3.4 with postcss-nesting |
| State | a hand-rolled broadcast channel between Rust and the front end (`ChannelMessage`, `EndpointInfo`) plus `notify-rust` for system notifications. **No** Pinia/Vuex. |
| Linux integration | tray icon via `libayatana-appindicator`, mDNS via a fork of `mdns-sd`, BLE via `bluer`/`btleplug` |
| Tests | one trivial `example.test.ts`; no protocol tests |

**Verified** by reading `app/main/package.json`, `app/main/src-tauri/Cargo.toml`
and `app/main/src-tauri/tauri.conf.json`.

### 1.3 Existing working features

* Receiving and sending files, text and Wi-Fi payloads over the Google Quick
  Share (Nearby Connections) protocol.
* mDNS service advertisement with visibility control (visible / invisible /
  temporarily) and BLE advertising so Android notices the desktop.
* A system notification when another device wants to send, with Accept/Reject
  actions handled back into the protocol.
* Download-path selection and a rotating log file.
* Minimize-to-tray and single-instance behaviour.

### 1.4 What was reused

* **Everything protocol-related.** `core/quickshare/` is a direct copy of
  `core_lib` with the crate renamed, the `RQS` struct renamed to
  `QuickShareService`, and the npm package renamed. The `.proto` files, the
  build script and the protocol logic are unchanged.
* **The Tauri/Vue/Tailwind stack** was kept, rather than starting a new front
  end, so the application keeps rquickshare's shape.
* The `ChannelMessage` protocol between Rust and the front end is unchanged, so
  the existing Quick Share screens and notification flow continue to work.

### 1.5 What was *not* reused, and why

* The rquickshare front end. The supplied Pixel Connect designs are a different
  and much more refined UI than upstream rquickshare's, and V1 needs screens
  upstream does not have (Devices, Notifications shade). The Tailwind token
  system and component conventions were reused; the components were rewritten.
* `Rename_build.sh` and `rename_build.sh`: brand-renaming scripts that are not
  relevant to a project that has its own identity from the start.

---

## 2. KDE Connect desktop (kdeconnect-kde)

### 2.1 Core module layout

`core/` is the `kdeconnectcore` library. The modern names are:

| File | Role |
|---|---|
| `networkpacket.{h,cpp}` | the wire protocol |
| `deviceinfo.h` | `DeviceInfo` plus the three identity-packet builders |
| `sslhelper.{h,cpp}` | certificate and key generation |
| `kdeconnectconfig.{h,cpp}` | config directory, trust store, key management |
| `daemon.{h,cpp}` | D-Bus `org.kde.kdeconnect.daemon` |
| `device.{h,cpp}` | D-Bus `org.kde.kdeconnect.device` |
| `kdeconnectplugin.{h,cpp}` | plugin base class |
| `backends/lan/` | `lanlinkprovider`, `landevicelink`, `server`, `uploadjob`, `compositeuploadjob`, `avahidiscovery`, `mdnsdiscovery` |
| `backends/bluetooth/` | `bluetoothlinkprovider` and its multiplexer |
| `backends/loopback/` | test-only, disabled by default |

Plugins live in `plugins/` and are listed with a plain `add_subdirectory` block,
so a subset can be built. Each plugin ships a JSON descriptor whose
`X-KdeConnect-SupportedPacketType` / `X-KdeConnect-OutgoingPacketType` arrays
become the advertised capabilities.

**Verified** by reading `core/CMakeLists.txt`, `plugins/CMakeLists.txt` and the
plugin headers.

### 2.2 Packet framing — the single most important finding

There is **no length prefix and no packet type byte**. Every packet is one line
of compact UTF-8 JSON terminated by a single `\n`:

```cpp
// core/networkpacket.cpp
QByteArray NetworkPacket::serialize() const
{
    QJsonObject obj;
    obj.insert(QLatin1String("id"), QDateTime::currentMSecsSinceEpoch());
    obj.insert(QLatin1String("type"), m_type);
    obj.insert(QLatin1String("body"), QJsonObject::fromVariantMap(m_body));
    if (hasPayload()) {
        obj.insert(QLatin1String("payloadSize"), m_payloadSize);
        obj.insert(QLatin1String("payloadTransferInfo"),
                   QJsonObject::fromVariantMap(m_payloadTransferInfo));
    }
    QByteArray json = QJsonDocument(obj).toJson(QJsonDocument::Compact);
    if (!json.isEmpty()) json.append('\n');
    return json;
}
```

Reading is `while (m_socket->canReadLine())`, capped at `MAX_PACKET_SIZE`
(32 MiB; 8 KiB for identity packets). The old binary protocol with the
`kdeconnect 4` magic header and numeric packet-type bytes was removed in v7.

**Verified** in `core/networkpacket.cpp` and
`core/backends/lan/landevicelink.cpp`.

### 2.3 Packet types actually used

Core: `kdeconnect.identity`, `kdeconnect.pair`.
Plugins Pixel Connect needs: `kdeconnect.clipboard`,
`kdeconnect.clipboard.connect`, `kdeconnect.clipboard.file` (desktop only),
`kdeconnect.share.request`, `kdeconnect.share.request.update`,
`kdeconnect.ping`, `kdeconnect.battery`, `kdeconnect.connectivity_report`,
`kdeconnect.notification`, `kdeconnect.notification.action`.

There is **no** numeric `Packet::Type` enum; the type is a `QString`.

### 2.4 Identity, discovery and ports

```cpp
// core/deviceinfo.h — the authoritative v8 identity
NetworkPacket DeviceInfo::toIdentityPacket()
{
    NetworkPacket np(PACKET_TYPE_IDENTITY);
    np.set(QStringLiteral("deviceId"), id);
    np.set(QStringLiteral("deviceName"), name);
    np.set(QStringLiteral("deviceType"), type.toString());
    np.set(QStringLiteral("protocolVersion"), protocolVersion);   // = 8
    np.set(QStringLiteral("incomingCapabilities"), incomingCapabilities.values());
    np.set(QStringLiteral("outgoingCapabilities"), outgoingCapabilities.values());
    return np;
}
```

* UDP discovery: broadcast to port **1716**, packet carries
  `deviceId`, `deviceName`, `protocolVersion`, `tcpPort`.
* TCP listen: linear scan from **1716** to **1764**.
* mDNS: type **`_kdeconnect._udp`**, instance name = device id, TXT records
  `id`, `name`, `type`, `protocol`.
* Device ids must match `^[a-zA-Z0-9_-]{32,38}$`.
* Device names are sanitised: `"',;:.!?()[]<>` removed, truncated to 32 chars.

**Verified** in `core/deviceinfo.h`, `core/backends/lan/lanlinkprovider.h` and
`core/backends/lan/avahidiscovery.cpp`.

### 2.5 Transport security

* Keys are **EC P-256 (secp256r1)**, generated with
  `EVP_PKEY_CTX_set_ec_paramgen_curve_nid(pctx, NID_X9_62_prime256v1)`.
* Certificates are **self-signed**, subject `CN=<deviceId>, O=KDE,
  OU=KDE Connect`, valid from one year ago to ten years ahead, serial number
  160 random bits, signed with **SHA-512**.
* TLS role inversion: whoever **opens** the TCP connection is the TLS
  **server**.
* Identity is exchanged **twice**: once in cleartext as the first TCP frame
  (with `targetDeviceId` / `targetProtocolVersion`), then again inside TLS with
  the full field set.
* Anti-spoofing: the peer certificate's CommonName must equal the announced
  `deviceId`, otherwise the socket is aborted:

```cpp
// core/backends/lan/lanlinkprovider.cpp
QString certDeviceId = socket->peerCertificate().subjectDisplayName();
if (deviceInfo.id != certDeviceId) {
    socket->abort();
    return;
}
```

* Trust store: `~/.config/kdeconnect/trusted_devices`, an INI file that pins
  each device's certificate PEM. Paired peers get `VerifyPeer` with the pinned
  certificate as CA; unpaired peers get `QueryPeer`.

### 2.6 Pairing

Only three packet shapes exist:

| Direction | Body |
|---|---|
| request pairing | `{"timestamp": <unix seconds>, "pair": true}` |
| accept | `{"pair": true}` |
| cancel / unpair / reject / timeout | `{"pair": false}` |

Timeout 30 s; clock tolerance 1800 s. The human-verifiable code is
`PairingHandler::verificationKey()`:

```cpp
auto a = ownCertificate().publicKey().toDer();
auto b = peerCertificate().publicKey().toDer();
if (a < b) std::swap(a, b);
QCryptographicHash hash(QCryptographicHash::Sha256);
hash.addData(a); hash.addData(b); hash.addData(QByteArray::number(timestamp));
return hex(result).left(8).toUpper();
```

On success, `Device::pairingHandler_pairingSuccessful()` pins the peer's
certificate via `KdeConnectConfig::addTrustedDevice`.

**Verified** in `core/backends/pairinghandler.cpp` and `core/device.cpp`.

### 2.7 Payload transfers

A file share is a header packet with `payloadSize` and
`payloadTransferInfo: {"port": N}`; the bytes then travel on a **second**
connection to that port with no framing. `CompositeUploadJob` binds a
`QTcpServer`, sends the header, waits for the receiver to connect, and streams
the file. Receiving works the same way in reverse.

### 2.8 Clipboard implementation

`plugins/clipboard/clipboardplugin.cpp` and
`plugins/clipboard/clipboardlistener.cpp`:

* Reads through `KSystemClipboard` (`clipboard->changed`).
* Password detection via the `x-kde-passwordManagerHint` MIME hint.
* Sends `kdeconnect.clipboard` `{content}` or
  `kdeconnect.clipboard.connect` `{content, timestamp}`.
* On connect it immediately sends a `kdeconnect.clipboard.connect` with the
  current content and timestamp.
* **Loop avoidance** is a monotonic local timestamp: a `clipboard.connect` whose
  `timestamp` is `0` or older than the local one is dropped. Combined with the
  listener's own "same content" early-return, A→B→A ping-pong cannot happen.
* Incoming `clipboard.file` paths are checked with `QDir::cleanPath` and a
  `startsWith(tmpDir + "/")` test to stop traversal.
* Config keys: `autoShare` (default true), `sendPassword` (default true),
  `maxClipboardFileSizeMB` (default 50).

### 2.9 Notification implementation

`plugins/notifications/notificationsplugin.{h,cpp}` implements
`org.kde.kdeconnect.device.notifications` with `activeNotifications()`,
`sendReply(ss)` and `sendAction(ss)`, and handles
`kdeconnect.notification.request|reply|action`. The reverse direction is
`plugins/sendnotifications/` which emits `kdeconnect.notification`.

### 2.10 Nautilus integration

`nautilus-extension/kdeconnect-share.py` is a Nautilus 4 `MenuProvider`. It:

* listens for `deviceListChanged` on `org.kde.kdeconnect.daemon`;
* lists devices with `deviceNames((bb))`, both flags true;
* calls `shareUrl(s)` on `org.kde.kdeconnect.device.share` per file;
* offers only regular files (directories are skipped).

### 2.11 D-Bus surface

`daemon` (on `/modules/kdeconnect`) exposes `selfId`, `devices(bb)`,
`deviceNames(bb)`, `deviceIdByName`, `linkProviders`, `setLinkProviderState`,
`sendSimpleNotification`; devices live on
`/modules/kdeconnect/devices/<deviceId>[/<plugin>]`. `kdeconnect-cli` is a thin
wrapper over exactly these interfaces.

### 2.12 Build dependencies

Qt >= 6.7, KF6 >= 6.0, ECM; the LAN backend needs `Qt6::Network`,
`Qt6::DBus`, `KF6::DBusAddons`, `KF6::I18n`, `KF6::Notifications`,
`KF6::KIO`, `KF6::Solid`, `KF6::CoreAddons`, `KF6::ConfigWidgets`,
`KF6::Crash`, `KF6::ItemViews`, `KF6::IconThemes`, `KF6::WindowSystem`,
`KF6::ColorScheme`, `KF6::GuiAddons`, `KF6::WidgetsAddons`, OpenSSL and Avahi
(linux) or the mDNS wrapper.

**Verified** from `CMakeLists.txt`.

---

## 3. KDE Connect Android

### 3.1 Package and structure

`applicationId = org.kde.kdeconnect_tp`, `minSdk 23`, `targetSdk 37`, Kotlin,
a single Gradle module. The `Device`/`LanLink`/`LanLinkProvider`/
`PairingHandler` classes live under `src/main/java/org/kde/kdeconnect/`.

### 3.2 Protocol parity

`NetworkPacket.kt` mirrors the desktop framing exactly, including the
slash-unescaping needed to match Qt's JSON output:

```kotlin
fun serialize(): String {
    val jo = JSONObject()
    jo.put("id", System.currentTimeMillis())
    jo.put("type", type)
    jo.put("body", mBody)
    ...
    // QJson does not escape slashes, but Java JSONObject does.
    return jo.toString().replace("\\/", "/") + "\n"
}
```

`PROTOCOL_VERSION = 8`, the same ports (`UDP_PORT = 1716`,
`MIN_PORT..MAX_PORT = 1716..=1764`), and the same mDNS/UDP discovery. Android
adds two things the desktop does not have: `TrustedNetworkHelper` (unpaired
devices on untrusted networks are ignored) and stricter private-address
filtering.

### 3.3 Background service

`BackgroundService` is a `foregroundServiceType="connectedDevice"` service
returning `START_STICKY`, holding a persistent foreground notification and a
`ConnectivityManager.NetworkCallback` for non-cellular transports (Wi-Fi, VPN,
Ethernet, Bluetooth, USB, lowpan). Doze is handled by the foreground service
plus the network callback; there is no wake-lock in the service itself.

### 3.4 Android clipboard restrictions

`plugins/clipboard/ClipboardPlugin.kt` works around the Android 10 foreground
restriction in three documented ways:

1. A `READ_LOGS` + logcat watch for `ClipboardService` errors, which launches
   a transparent activity to gain focus.
2. `ClipDescription.EXTRA_IS_SENSITIVE` to avoid forwarding passwords.
3. A manual "Send clipboard" affordance (UI button, menu entry, quick-settings
   tile) when automatic syncing is unavailable.

The loop breaker is the same timestamp comparison as the desktop.

### 3.5 Common misstatements, corrected

These are wrong for protocol 8 and were checked against the code:

| Common claim | Reality |
|---|---|
| "KDE Connect frames packets with a 4-byte length prefix" | No length prefix at all; newline-terminated JSON. |
| "There is a `kdeconnect 4` magic header on the discovery broadcast" | Removed in v7. |
| "Packets start with a numeric type byte" | The type is a `QString`. |
| "Pairing exchanges RSA public keys in the pair packet" | `kdeconnect.pair` carries only `pair` and `timestamp`. RSA generation survives only for legacy key loading. |
| "Keys are derived with AES/HMAC from an RSA shared secret" | No key derivation exists; confidentiality comes entirely from TLS. |
| "The device certificate carries the device name" | The CN is the **device id**. |
| "Discovery uses UDP 1740" | It uses 1716. |
| "The share packet contains a `files` array and `filesize`" | It carries a single `filename` (plus injected `numberOfFiles`/`totalPayloadSize`). |
| "Desktop Bluetooth is OBEX object push" | It is a single RFCOMM connection with a custom multiplexing protocol. |

---

## 4. Reuse decisions

| Subsystem | Decision | Rationale |
|---|---|---|
| Quick Share protocol | **Copied verbatim** into `core/quickshare` | It is working, it is Rust, and rewriting it would risk the one feature the user already relies on. |
| Tauri + Vue + Tailwind stack | **Kept** | Reusing rquickshare's front-end architecture rather than starting a new one. |
| rquickshare front-end components | **Rewritten** to the supplied design | The supplied design is the acceptance reference and needs screens upstream lacks. |
| KDE Connect core (C++/Qt) | **Not bundled** | See §5. |
| KDE Connect protocol | **Ported to Rust** in `core/kdeconnect` | See §5. |
| KDE Connect Android app | **Reused unchanged for compatibility testing** | It speaks the same protocol; a companion is not required for V1. |
| Nautilus extension | **Adapted** to talk to the Pixel Connect socket | Same extension mechanism, different backend. |

---

## 5. Why the KDE Connect backend was ported rather than bundled

The brief asked, in order of preference, to (1) bundle KDE Connect core
components, (2) compile them from upstream as part of the build, or (3) port the
minimum required protocol to Rust. Here is the assessment.

**Bundling a renamed `kdeconnectd` is not viable.** KDE Connect's daemon claims
the well-known D-Bus name `org.kde.kdeconnect`, stores its state in
`~/.config/kdeconnect`, and registers plugin D-Bus services generated from the
plugin headers. A byte-identical copy would collide with any real KDE Connect
installation, which is precisely what Pixel Connect must not require. Making it
independent means re-branding the daemon, the CLI, every plugin's
`KService` metadata and its config path — that is a fork, not a bundle.

**Building it from source inside Pixel Connect's own build is not practical for
V1.** The full dependency set is Qt >= 6.7 plus twelve KF6 frameworks
(I18n, DBusAddons, IconThemes, Notifications, KIO, ColorScheme, Service, Solid,
Kirigami, People, WindowSystem, GuiAddons, Crash, ItemModels,
WidgetsAddons), ECM, and Avahi or the bundled mDNS wrapper. That is a very large
build for something that must then still be re-branded as above, and it would
make Pixel Connect a KDE application — which the brief explicitly rules out by
requiring an independent identity. A Tauri application cannot embed a Qt event
loop; it would have to be a separately packaged, separately supervised helper,
which reintroduces the "do not rely on a separately installed KDE Connect"
problem the brief prohibits.

**Porting the protocol is what the brief's option 3 describes, and it is the
right call here.** KDE Connect's LAN backend is self-contained: JSON over TLS
with certificate pinning. It has no dependency on KDE, Qt or KF at all. Porting
it gives Pixel Connect:

* its own identity, config directory and process — no D-Bus name to collide;
* no Qt/KF runtime dependency (it needs `rustls` and a system OpenSSL-free
  TLS stack);
* a single process, so there is no helper to supervise, package or keep alive;
* full control over the UI, which the brief requires.

What was *not* ported, deliberately: the SFTP, MPRIS, SMS, contacts, telephony,
digitizer, virtual-monitor, find-my-phone, lock, presenter, remotecommands and
runcommand plugins. They are not needed for V1 and each would drag in a large
surface of protocol behaviour to test.

The port is wire-faithful rather than clean-room: every packet name, body key,
port, TLS requirement and pairing rule was read out of the upstream sources and
reproduced exactly. See `NOTICE.md` for the attribution.

### Verification performed

`core/kdeconnect/tests/interop.rs` starts two full services in one process and
exercises the real stack over loopback TCP: discovery, the cleartext identity
frame, the TLS handshake in both roles, CommonName pinning, the in-TLS identity
exchange, pairing with a verification key, certificate pinning on both sides,
clipboard delivery, and a 512 KiB file transfer verified byte-for-byte.

**This is verified. Interoperability with kdeconnect-android is *not* verified**,
because no Pixel phone was available in this environment. That gap is recorded
in `docs/architecture/v1-status.md`.

---

## 6. Bluetooth — findings

**Desktop** (`core/backends/bluetooth/`): not OBEX object push. It is a single
RFCOMM connection to SDP service UUID
`185f3df4-3268-4e3f-9fca-d4d5059915bd` named "KDE Connect", carrying a custom
multiplexing protocol — `1 byte` message type, `2 bytes` big-endian length,
`16 bytes` channel UUID, then data — with message types
`PROTOCOL_VERSION=0`, `OPEN_CHANNEL=1`, `CLOSE_CHANNEL=2`, `READ=3`,
`WRITE=4` and a default channel UUID `a0d0aaf4-1072-4d81-aa35-902a954b1266`.
Because the RFCOMM link is not TLS, the identity packet carries an extra PEM
`certificate` field.

It is **disabled by default upstream**:
`KdeConnectConfig::disabledLinkProviders()` returns `{"bluetooth", "loopback"}`,
citing bugs 520578 and 513536.

**Android**: classic RFCOMM. The client requires OS-level Bluetooth pairing
first (it iterates `bondedDevices` on `BluetoothDevice.ACTION_UUID` and calls
`fetchUuidsWithSdp`), and the setting is off by default.

**Assessment for V1.** A Bluetooth transport would need, on both sides: the
multiplexer, a non-TLS link, the PEM certificate in the identity packet, and
enabling a backend that upstream disables because it "causes issues with other
BT devices". That is a substantial, fragile subsystem with no way to test here.
Pixel Connect therefore ships the transport abstraction but does **not**
implement Bluetooth; the Devices screen says so explicitly rather than showing
a control that would not work.

---

## 7. Wi-Fi Direct — findings

**KDE Connect has no Wi-Fi Direct support.** Verified by:

* a case-insensitive grep for `wifi.?p2p`, `wifip2p`, `\bwfd\b`,
  `direct.?connect` across the whole checkout returning nothing relevant;
* the `add_subdirectory` list containing only `lan`, `bluetooth` and
  `loopback` backends;
* `Daemon::linkProviders()` being able to return only those three;
* Android's `BackgroundService.registerLinkProviders()` adding only
  `LanLinkProvider` and `BluetoothLinkProvider`. The only Wi-Fi-adjacent Android
  code is `TRANSPORT_WIFI`/`TRANSPORT_WIFI_AWARE` for *observing* the network
  and `WIFI_STATE_CHANGED` for rediscovery.

Every KDE Connect transport is IP-based or RFCOMM. There is no PnP GO/peer
negotiation, no WFD, no P2P group anywhere.

**Feasibility of adding it on Fedora GNOME/Wayland.** An Android phone in
Wi-Fi Direct P2P Group Owner mode hands out a group IP over a virtual
interface that Linux normally models with `wpa_supplicant`
`p2p_*` / NetworkManager's Wi-Fi P2P support, plus `mac80211_hwsim` or real
driver `WIFI_DIRECT` support for the laptop to act as a Peer. On Fedora this
requires: a wireless adapter whose driver supports P2P; NetworkManager's
`wifi-p2p` plugin; and pairing the P2P GO on the phone. Many modern Intel and
MediaTek adapters advertise `WIFI_DIRECT` but the P2P GO role is frequently
broken or driver-specific, and NetworkManager's P2P support is best-effort and
incomplete. More importantly, it would need a *new* transport in Pixel Connect
plus a matching change in the phone app, because the desktop and Android apps
both only speak TCP/IP — the phone would have to enable a P2P group that the
laptop is already connected to.

**Decision.** Not implemented in V1. The transport abstraction exists
(`Transport::{Lan, Bluetooth, None}`), the Devices screen states the situation
plainly, and no status is ever faked. A proof of concept would need real
hardware and a second engineer; the honest answer is that this is a project of
its own.

---

## 8. Packaging and runtime dependencies

Pixel Connect's runtime dependencies are deliberately small:

| Dependency | Required? | Why |
|---|---|---|
| `webkit2gtk4.1`, `gtk3`, `libsoup3` | yes | Tauri/wry |
| `libappindicator-gtk3`, `dbusmenu-libs` | yes | tray icon |
| `libgcc`, `glibc` | yes | — |
| `adb` (`android-tools`) | optional | ADB device listing; the UI explains its absence |
| `scrcpy` | optional | screen mirroring |
| `openssh-clients` | optional | SSH integration |
| `nautilus-python`, `python3-nautilus` | optional | top-level Nautilus context-menu entry |
| `avahi` / `mdns` | via `mdns-sd` (Rust, no C dependency) | discovery |

**KDE Connect, `kdeconnectd`, `kdeconnect-cli` and GSConnect are not
dependencies and are not invoked.** The only reason KDE Connect appears in this
document is protocol compatibility.

---

## 9. Technical limitations found during the audit

1. **UDP 1716 is a single fixed port.** Two KDE Connect implementations on one
   host collide. Pixel Connect degrades to mDNS-only discovery and says so
   rather than refusing to start. (Observed live on the build machine, where a
   real `kdeconnectd` already held the port.)
2. **Android 10+ forbids background clipboard reads.** There is no way around
   this that does not involve either foreground interaction or the logcat
   workaround upstream uses. Pixel Connect does not use the logcat trick.
3. **Payload transfers invert the TLS roles again** relative to the control
   channel, because the receiver dials. Getting this wrong fails in a way that
   looks like a hang; it is pinned by an integration test.
4. **Directory shares are flattened**, not recursive, upstream. Pixel Connect
   matches that and sends each file individually.
5. **`kdeconnect.clipboard.file` has no Android counterpart**, so file/image
   clipboard sync is desktop-only upstream and is not offered here.
6. **Device ids are 32–38 characters**, and KDE Connect *rejects* identity
   packets that violate this. An incorrectly generated id makes the device
   invisible rather than producing an error, which is easy to misdiagnose.