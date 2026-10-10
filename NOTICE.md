Pixel Connect
Copyright (C) 2026 Pixel Connect contributors

Licensed under the GNU Affero General Public License, version 3 or later.
See licenses/AGPL-3.0.txt for the full text.

--------------------------------------------------------------------------------
Third-party works
--------------------------------------------------------------------------------

## 1. rquickshare (Google Quick Share / Nearby Connections)

    https://github.com/Martichou/rquickshare
    Copyright (C) Martin ANDRE and the rquickshare contributors
    Licensed under the GNU Affero General Public License v3.0

`core/quickshare/` is a derivative work of rquickshare's `core_lib` crate
(renamed to `pixel_connect_quickshare`). The following are reused:

  * the Google Quick Share / Nearby Connections wire protocol:
    mDNS service advertisement, BLE advertising, the UKEY2 / PAKE handshake,
    secure-message framing and the payload transfer format
  * the protobuf definitions under `src/proto_src/` (verbatim)
  * the build script that compiles them
  * the TypeScript bindings under `bindings/`

The following were changed:

  * the crate is named `pixel_connect_quickshare` instead of `rqs_lib`
  * the public service struct is named `QuickShareService` instead of `RQS`
  * the log environment variable is `PIXEL_CONNECT_LOG` instead of `RQS_LOG`
  * the npm package is `@pixelconnect/quickshare` instead of
    `@martichou/core_lib`

No protocol behaviour was modified. Pixel Connect needs rquickshare's licence
and attribution to remain intact, which is why Pixel Connect is AGPL-3.0.

The original licence text is preserved in licenses/AGPL-3.0.txt.

## 2. KDE Connect (device integration protocol)

    https://github.com/KDE/kdeconnect-kde
    https://github.com/KDE/kdeconnect-android
    Copyright (C) the KDE Connect contributors
    Licensed under GPL-2.0-only OR GPL-3.0-only OR LicenseRef-KDE-Accepted-GPL

`core/kdeconnect/` implements the KDE Connect **protocol**, version 8. It is a
reimplementation in Rust: no KDE Connect source code is copied into this
project. What is reproduced is the wire contract, which must match exactly for
interoperability:

  * newline-terminated JSON packet framing (`core/networkpacket.cpp`,
    `NetworkPacket.kt`)
  * packet names and body keys (`core/networkpackettypes.h`, plugin headers)
  * the identity packet fields and device-id rule
    (`core/deviceinfo.h`, `DeviceInfo.kt`)
  * UDP port 1716 and the TCP port range 1716..=1764
    (`core/backends/lan/lanlinkprovider.h`)
  * mDNS service type `_kdeconnect._udp` with TXT records `id`, `name`, `type`,
    `protocol` (`core/backends/lan/avahidiscovery.cpp`)
  * the TLS model: self-signed EC (P-256) certificates whose CommonName is the
    device id, pinned per device (`core/sslhelper.cpp`,
    `core/backends/lan/lanlinkprovider.cpp`)
  * the TLS role inversion rule — whoever opens the TCP connection is the TLS
    server
  * the pairing packet forms and the 8-character verification key
    (`core/backends/pairinghandler.cpp`, `PairingHandler.kt`)
  * payload transfer on a second connection announced by
    `payloadTransferInfo.port` (`core/backends/lan/compositeuploadjob.cpp`)
  * the clipboard timestamp loop-breaker and share packet body keys
    (`plugins/clipboard/`, `plugins/share/`)

The KDE Connect licence permits this reuse. Pixel Connect is distributed under
AGPL-3.0, which is compatible with the GPL terms of those works; the relevant
GPL text is in licenses/GPL-2.0.txt.

## 3. Nautilus extension mechanism

    `packaging/nautilus/pixel-connect-share.py` is adapted from
    https://github.com/KDE/kdeconnect-kde `nautilus-extension/kdeconnect-share.py`
    Copyright (C) 2011-2016 Aleksey Filippov and the KDE Connect contributors
    Licensed GPL-2.0-only OR GPL-3.0-only OR LicenseRef-KDE-Accepted-GPL

It uses the same Nautilus `MenuProvider` API but talks to the Pixel Connect
backend socket instead of `org.kde.kdeconnect`, so KDE Connect does not need to
be installed.

## 4. Rust and npm dependencies

Pixel Connect depends on the crates and npm packages listed in the various
`Cargo.toml` / `package.json` files. Each retains its own licence. The most
notable are:

  * `rustls` (Apache-2.0 OR ISC OR MIT), `rcgen` (MIT OR Apache-2.0),
    `x509-parser` (Apache-2.0), `tokio` (MIT), `serde` (MIT OR Apache-2.0),
    `mdns-sd` (MIT OR Apache-2.0), `prost` (Apache-2.0),
    `p256` (Apache-2.0 OR MIT), `notify-rust` (CC0-1.0 OR MIT),
    `arboard` (MIT OR Apache-2.0), `wl-clipboard-rs` (MIT)
  * `vue`, `pinia`, `vite`, `tailwindcss`, `vitest` (all MIT)

## 5. Design reference

The user interface implements the supplied Pixel Connect design files in
`Designs/` (provided with this project). No third-party artwork is included.