# Transport findings: Bluetooth and Wi-Fi Direct

V1 reported both of these as unavailable based on reading upstream code. That
was the right conclusion but the wrong basis — it was inferred from source code
rather than measured on real hardware, and one of the two conclusions turned out
to be closer than the original wording suggested.

This document records what was actually measured, on what hardware, and what
would be required to implement each.

Every command below was run on the build machine. Results are quoted verbatim.

---

## 1. Wi-Fi Direct

### 1.1 The scenario

The laptop is on isolated university Wi-Fi; the Pixel is on a different
network. The two LANs cannot reach each other, so neither Quick Share nor KDE
Connect can connect over IP as they normally would. A Wi-Fi Direct Group Owner
(typically the phone) creates a direct link with its own IP network, and both
protocols would then work unchanged **if** an IP link could be established.

### 1.2 What the hardware supports

```console
$ iw list | grep -A3 "Supported interface modes"
		 * IBSS
		 * managed
		 * AP
...
		 * P2P-client
		 * P2P-GO
		 * P2P-device
```

```console
$ readlink -f /sys/class/net/wlo1/device/driver
/sys/bus/pci/drivers/iwlwifi

$ lspci | grep -i network
00:14.3 Network controller: Intel Corporation 700 Series Chipset Family CNVi
        Wi-Fi (rev 11)
```

**The hardware and driver are capable.** This is an Intel `iwlwifi` adapter and
it advertises all three P2P interface modes. V1's statement that "many modern
Intel adapters implement the GO role unreliably" is not supported by anything on
this machine.

### 1.3 What the Fedora networking stack offers

NetworkManager is running and, tellingly, has **already created a P2P device**:

```console
$ nmcli -t -f DEVICE,TYPE,STATE,CONNECTION dev
wlo1:wifi:connected:Jiovattikutiraj4g
p2p-dev-wlo1:wifi-p2p:disconnected
```

So NetworkManager's internal P2P machinery is present and has instantiated the
`p2p-dev-wlo1` interface. But none of it is reachable:

```console
$ nmcli --help | grep -i p2p
                       (no output; exit 1)

$ gdbus introspect --system --dest org.freedesktop.NetworkManager \
      --object-path /org/freedesktop/NetworkManager/Client | grep -i p2p
                       (no output)

$ gdbus introspect --system --dest org.freedesktop.NetworkManager \
      --object-path /org/freedesktop/NetworkManager/P2P
node /org/freedesktop/NetworkManager/P2P {
};

$ nmcli -t dev show wlo1 | grep -i p2p
                       (no output)

$ nmcli connection add type wifi-p2p p2p-probe ifname wlo1
Error: invalid <setting>.<property> 'p2p-probe'.
```

NetworkManager 1.56.1 creates the P2P device but exposes **no P2P API at all**:
no D-Bus methods, no `nmcli` subcommand, no `wifi-p2p` connection type, no P2P
properties. There is no supported way to make it discover peers or join a group.

`wpa_supplicant-2.11` is installed and implements the P2P protocol, but
NetworkManager owns `wlo1`, and its supplicant control socket is not exposed at
the standard path:

```console
$ wpa_cli -i wlo1 p2p_listen
Failed to connect to non-global ctrl_ifname: wlo1  error: No such file or directory
```

Driving wpa_supplicant directly would mean taking `wlo1` away from
NetworkManager — which would drop the user's Wi-Fi connection — and creating the
`p2p-dev-*` interface requires privileges this environment does not have:

```console
$ id -u
1000
$ sudo -n true
sudo: a password is required
$ ip link add p2p-probe type dummy
RTNETLINK answers: Operation not permitted
$ iw dev wlo1 set type managed
command failed: Operation not permitted (-1)
```

### 1.4 KDE Connect itself

KDE Connect has no Wi-Fi Direct support in any version. Both the desktop and
Android implementations speak only TCP/IP and RFCOMM. There is no P2P GO/peer
negotiation, no WFD, no P2P group anywhere. The only Wi-Fi-adjacent code on the
Android side is *observing* the network (`TRANSPORT_WIFI`,
`TRANSPORT_WIFI_AWARE`).

So Wi-Fi Direct would not be "enabling a KDE Connect feature"; it would be a
brand-new transport in Pixel Connect plus a matching change on the phone.

### 1.5 Conclusion

| Question | Answer |
|---|---|
| Does the hardware support P2P? | **Yes** — P2P-client, P2P-GO, P2P-device |
| Does NetworkManager expose a usable P2P API? | **No** |
| Can wpa_supplicant be used instead? | Yes, but it needs root and conflicts with NetworkManager |
| Does KDE Connect support P2P? | **No**, in any version |
| Could a proof of concept be built and tested here? | **No** — no root, and no phone to connect to |

**Not implemented.** Three independent blockers, of which two are environmental
and one is a matter of scope: NetworkManager's missing API, the absence of root
and of a second device in this environment, and the fact that neither KDE
Connect nor Android KDE Connect has any P2P transport to reuse.

The transport is declared in `core/kdeconnect/src/transport.rs` with its
blocker recorded, so a future implementation has a place to land. The Devices
screen shows it as *Not implemented* with this explanation — not as a dead
button.

**The isolated-network scenario is not solved.** V1 said so; V2 confirms it with
measurements rather than inference.

---

## 2. Bluetooth

### 2.1 What upstream implements

KDE Connect's Bluetooth backend is **not** OBEX object push, which is a common
misstatement. It is a single RFCOMM connection:

```console
$ grep -rn "185f3df4" /tmp/opencode/src/kdeconnect-kde/core/backends/bluetooth/
bluetoothlinkproviderimpl.cpp:20:
    , mServiceUuid(QBluetoothUuid(QStringLiteral("185f3df4-3268-4e3f-9fca-d4d5059915bd")))
bluetoothlinkproviderimpl.cpp:66:
    mKdeconnectService = mBluetoothServer->listen(mServiceUuid, QStringLiteral("KDE Connect"));
```

carrying a custom multiplexing protocol on top: a 1-byte message type, a 2-byte
big-endian length, a 16-byte channel UUID, then the data. Message types are
`PROTOCOL_VERSION=0`, `OPEN_CHANNEL=1`, `CLOSE_CHANNEL=2`, `READ=3`, `WRITE=4`,
with a default channel UUID of `a0d0aaf4-1072-4d81-aa35-902a954b1266`.

Because RFCOMM is not TLS, the identity packet carries an extra PEM
`certificate` field.

### 2.2 Why upstream disables it

`KdeConnectConfig::disabledLinkProviders()` returns `{"bluetooth", "loopback"}`,
citing KDE bugs **520578** and **513536** — it is off by default because it
"causes issues with other BT devices".

### 2.3 What the Android side requires

The KDE Connect Android app requires **operating-system-level Bluetooth pairing
with the computer first** (it iterates `bondedDevices` on
`BluetoothDevice.ACTION_UUID` and calls `fetchUuidsWithSdp`), and its own
Bluetooth transport is off by default. So even with a working Linux
implementation, the user must pair at the OS level and enable a setting on the
phone before anything connects.

### 2.4 What this machine provides

```console
$ rpm -q bluez bluez-libs
bluez-5.87+1.git8750129efca8-1.fc44.x86_64
bluez-libs-5.87+1.git8750129efca8-1.fc44.x86_64

$ bluetoothctl list
Controller 30:E3:A4:0D:55:B5 fedora [default]
```

BlueZ is present with one adapter, and the profile registration API exists:

```console
$ busctl --system introspect org.bluez /org/bluez org.bluez.ProfileManager1
NAME                  TYPE              SIGNATURE  RESULT  FLAGS
.RegisterProfile      method            osa{sv}    -        -
.UnregisterProfile    method            o          -        -
```

Creating an `AF_BLUETOOTH`/`BTPROTO_RFCOMM` socket from an unprivileged process
also succeeds. So the Linux side of an RFCOMM transport is genuinely buildable
here — `bluer`, already a rquickshare dependency, can drive the BlueZ D-Bus
work.

### 2.5 The decisive blocker

**No Pixel phone was attached**, so there is no peer to:

- establish the OS-level Bluetooth pairing the Android app demands,
- exchange SDP records and confirm the `185f3df4-…` service resolves,
- exercise the multiplexing handshake,
- or verify a single byte of clipboard or file traffic over it.

The task brief is explicit on this point: *"Implement only the behaviors that are
technically supported and actually tested. Do not claim that Bluetooth alone
provides full Quick Share, clipboard synchronization, or screen mirroring
connectivity without a functioning compatible transport."*

Shipping an untested multiplexer that upstream itself disables would be exactly
that claim. The blocker is the missing device, not missing code.

### 2.6 Conclusion

**Not implemented.** The Linux foundation is present and buildable; the blocker
is that nothing can be verified without a phone, on a transport that upstream
keeps disabled and Android gates behind an OS-level pairing the user has not
performed.

Declared in `core/kdeconnect/src/transport.rs` with the blocker recorded.

---

## 3. What would unblock each

### Wi-Fi Direct

1. Root on a machine whose adapter supports P2P (this one does).
2. Either a NetworkManager build with a usable P2P API, or a deliberate decision
   to let Pixel Connect own a `p2p-dev-*` interface via wpa_supplicant without
   NetworkManager.
3. A phone-side application that creates a P2P Group Owner. Neither KDE Connect
   nor the Android KDE Connect app does this, so a Pixel Connect Android
   companion would have to add it.
4. Once the IP link exists, **no protocol work is needed** — Quick Share and KDE
   Connect both run over plain IP.

That last point is worth emphasising: the hard part is the link, not the
payloads. A working P2P link would unlock every existing feature at once.

### Bluetooth

1. A Pixel attached over USB.
2. OS-level Bluetooth pairing between the phone and this computer.
3. Enable the Bluetooth transport in the Android app.
4. Then the RFCOMM multiplexer becomes testable.

Estimated work once a device is available: the multiplexer (about 400 lines),
BlueZ profile registration, the extra identity `certificate` field, and
integration tests that can finally run against a real peer.

---

## 4. Summary table

| Transport | Hardware OK | Stack OK | Testable here | Implemented | Blocker |
|---|---|---|---|---|---|
| LAN | yes | yes | yes | **yes** | — |
| Bluetooth | yes | yes (BlueZ) | **no** — no phone | no | no device to verify against; upstream-disabled; Android requires OS pairing |
| Wi-Fi Direct | yes | **no** — NetworkManager exposes no P2P API | **no** — needs root + phone | no | no NetworkManager P2P API; no root; no P2P transport in KDE Connect or its Android app |

Neither blocker is "we ran out of time". Both are concrete, measured, and
recorded in the code so the next engineer starts from evidence rather than from
a guess.