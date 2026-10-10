# Android companion

## Decision: not required for V1

Pixel Connect implements **KDE Connect protocol version 8**, which is exactly
what the current KDE Connect app for Android
(`org.kde.kdeconnect_tp`, F-Droid and Google Play) speaks. It therefore pairs
with Pixel Connect **unmodified** — there is nothing for a companion to add for
any V1 feature:

| V1 feature | Android client |
|---|---|
| Discovery | KDE Connect app (mDNS + UDP) |
| Pairing | KDE Connect app |
| Clipboard sync | KDE Connect app |
| Send/receive files | KDE Connect app |
| Battery, connectivity | KDE Connect app |
| Notifications | KDE Connect app |

Quick Share is entirely separate: it uses Google's own Quick Share app on the
phone and does not involve KDE Connect at all.

This is corroborated at build time: the running upstream `kdeconnectd` on the
build machine interoperated with Pixel Connect — discovery, TLS handshake,
identity exchange and capability negotiation all succeeded between two
implementations that share no code. See `docs/architecture/v1-status.md` §1.3.

**No Pixel phone was available during development**, so pairing with the actual
Android app is unverified. It is the first thing to check when a phone is
available.

---

## If a branded companion is later required

Fork kdeconnect-android (GPL-2.0-only OR GPL-3.0-only OR
LicenseRef-KDE-Accepted-GPL), keep the protocol implementation unchanged, and
change only branding and identity.

```bash
git clone https://github.com/KDE/kdeconnect-android.git
cd kdeconnect-android
```

### 1. Identity

In `build.gradle.kts`:

```kotlin
android {
    // NOT org.kde.kdeconnect_tp: this must be a distinct application id.
    namespace = "dev.pixelconnect.companion"
    defaultConfig {
        applicationId = "dev.pixelconnect.companion"
        versionName = "0.1.0"
    }
}
```

### 2. Signing

**Do not reuse upstream keys.** Create your own keystore:

```bash
keytool -genkey -v -keystore pixel-connect-release.jks \
  -alias pixel-connect -keyalg RSA -keysize 4096 -validity 10000
```

Add a `keystore.properties` (git-ignored):

```properties
storeFile=pixel-connect-release.jks
storePassword=…
keyAlias=pixel-connect
keyPassword=…
```

and read it in `build.gradle.kts`:

```kotlin
val keystoreProperties = java.util.Properties().apply {
    val f = rootProject.file("keystore.properties")
    if (f.exists()) f.inputStream().use { load(it) }
}

android {
    signingConfigs {
        create("release") {
            if (keystoreProperties.containsKey("storeFile")) {
                storeFile = rootProject.file(keystoreProperties["storeFile"] as String)
                storePassword = keystoreProperties["storePassword"] as String
                keyAlias = keystoreProperties["keyAlias"] as String
                keyPassword = keystoreProperties["keyPassword"] as String
            }
        }
    }
    buildTypes { release { signingConfig = signingConfigs.getByName("release") } }
}
```

### 3. Branding

Replace `src/main/res/mipmap-*/ic_launcher*` and the application label with the
Pixel Connect assets from `data/branding/`. Adjust the notification-channel
names in `NotificationHelper` if you want them distinct.

### 4. Do not change

* `src/main/java/org/kde/kdeconnect/` — the protocol implementation.
* `NetworkPacket.kt` — framing, packet types, body keys.
* `backends/lan/` — discovery, ports, TLS roles, certificate handling.
* `PairingHandler.kt` — the verification-key derivation.
* `plugins/clipboard/` and `plugins/share/`.

Changing either side of the protocol arbitrarily is how interoperability breaks.

### 5. Permissions the phone will ask for

| Permission | Why |
|---|---|
| `ACCESS_LOCAL_NETWORK` | reach the computer on the LAN |
| `INTERNET`, `ACCESS_NETWORK_STATE`, `CHANGE_NETWORK_STATE` | discovery and reconnection |
| `CHANGE_WIFI_MULTICAST_STATE` | mDNS replies |
| `ACCESS_WIFI_STATE`, `CHANGE_WIFI_STATE` | reacting to Wi-Fi changes |
| `POST_NOTIFICATIONS` (Android 13+) | foreground service |
| `FOREGROUND_SERVICE`, `FOREGROUND_SERVICE_CONNECTED_DEVICE` | staying alive |
| `BLUETOOTH_*` | only if a Bluetooth transport is ever added |

**Battery optimisation:** the service is a `connectedDevice` foreground service
and should be exempted from Doze for reliable background operation. On stock
Android, Settings → Apps → KDE Connect → Battery → *Unrestricted*. On some
OEM skins an explicit auto-start entry is also needed.

**Clipboard:** Android 10+ only lets a focused app read the clipboard. Upstream
works around this with a `READ_LOGS` + logcat trick. A companion should prefer
the explicit, supported affordances — a quick-settings tile and a foreground
"send clipboard" action — rather than replicating that hack.

### 6. Build

```bash
./gradlew assembleRelease
adb install -r app/build/outputs/apk/release/app-release.apk
```