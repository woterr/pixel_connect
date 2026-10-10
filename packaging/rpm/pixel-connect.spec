Name:           pixel-connect
Version:        0.1.0
Release:        1%{?dist}
Summary:        Pixel Connect — Quick Share, clipboard and device integration for Google Pixel phones

License:        AGPL-3.0
URL:            https://github.com/pixelconnect/pixel-connect
Source0:        %{name}-%{version}.tar.gz

BuildRequires:  cargo
BuildRequires:  rust
BuildRequires:  gcc-c++
BuildRequires:  make
BuildRequires:  pkgconfig(pkgconf)
BuildRequires:  pkgconfig(webkit2gtk4.1)
BuildRequires:  pkgconfig(gtk3)
BuildRequires:  pkgconfig(soup3)
BuildRequires:  pkgconfig(appindicator3-0.1)
BuildRequires:  pkgconfig(dbusmenu-glib-0.4)
BuildRequires:  pkgconfig(openssl)
BuildRequires:  openssl-devel
BuildRequires:  dbus-devel
BuildRequires:  libappindicator-gtk3-devel
BuildRequires:  librsvg2-devel
BuildRequires:  patchelf
BuildRequires:  nodejs
BuildRequires:  npm

Requires:       webkit2gtk4.1
Requires:       gtk3
Requires:       libsoup3
Requires:       libappindicator-gtk3
Requires:       dbusmenu-libs
Requires:       openssl-libs
Requires:       alsa-lib
Requires:       gdk-pixbuf2

# Optional at runtime: Pixel Connect detects and explains their absence, it
# does not hard-depend on them.
Recommends:      scrcpy
Recommends:      android-tools
Recommends:      nautilus-python
Recommends:      python3-nautilus
Recommends:      openssh-clients

%description
Pixel Connect unifies Google Pixel connectivity on Linux:

* Google Quick Share, using the Nearby Connections protocol implementation
  derived from rquickshare.
* KDE Connect protocol version 8 device integration — discovery, pairing,
  bidirectional clipboard synchronisation and file sharing — implemented in
  process, so KDE Connect, kdeconnectd, kdeconnect-cli and GSConnect do NOT
  have to be installed.
* A "Send to Pixel" entry for the GNOME Files (Nautilus) context menu.
* ADB device detection and scrcpy lifecycle management for screen mirroring.
* An optional, explicitly configured SSH client integration.

Everything works locally on the LAN; no cloud service, account or relay is
involved.

%prep
%autosetup -n -p

%build
export PROTOC="${PROTOC:-protoc}"
# The front end is built first so Tauri can embed it.
pushd app/main
%if ! [ -d node_modules ]
    npm install --no-audit --no-fund
%endif
npm run vite:build
popd

# A release build with the front end embedded.
pushd app/main/src-tauri
cargo build --release --locked
popd

%install
install -Dm0755 app/main/src-tauri/target/release/pixel-connect \
    %{buildroot}%{_bindir}/pixel-connect

install -Dm0644 packaging/nautilus/pixel-connect-share.py \
    %{buildroot}%{_datadir}/pixel-connect/nautilus/pixel-connect-share.py
install -Dm0755 packaging/nautilus/pixel-connect-send \
    %{buildroot}%{_datadir}/pixel-connect/pixel-connect-send

install -Dm0644 packaging/desktop/dev.pixelconnect.app.desktop \
    %{buildroot}%{_datadir}/applications/dev.pixelconnect.app.desktop
install -Dm0644 data/branding/pixel-connect-128.png \
    %{buildroot}%{_datadir}/icons/hicolor/128x128/apps/pixel-connect.png
install -Dm0644 data/branding/pixel-connect-symbolic.svg \
    %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/pixel-connect-symbolic.svg

install -Dm0644 LICENSE %{buildroot}%{_datadir}/licenses/pixel-connect/LICENSE
install -Dm0644 NOTICE.md %{buildroot}%{_datadir}/licenses/pixel-connect/NOTICE.md
install -Dm0644 README.md %{buildroot}%{_datadir}/doc/pixel-connect/README.md

# The Nautilus extension goes to the system location; the post-install script
# links it into the user's home so it works without root.
install -Dm0644 packaging/nautilus/pixel-connect-share.py \
    %{buildroot}%{_datadir}/nautilus-python/extensions/pixel-connect-share.py

%post
%{_bindir}/gtk-update-icon-cache -q %{_datadir}/icons/hicolor 2>/dev/null || :
%{_bindir}/update-desktop-database -q %{_datadir}/applications 2>/dev/null || :

# Offer the file-manager integration for the current user. This never requires
# KDE Connect and is fully reversible.
#
# The scriptlet runs as root, so everything it creates must be *done* as the
# user. Creating root-owned entries inside someone's home directory works, but
# leaves files the user cannot replace without sudo, which is both surprising
# and a needless privilege use on a desktop.
if [ "$1" = "1" ] && [ -n "$USER" ] && [ "$USER" != "root" ]; then
    _home="$(getent passwd "$USER" | cut -d: -f6)"
    if [ -n "$_home" ]; then
        if command -v runuser >/dev/null 2>&1; then
            _asuser="runuser -u $USER --"
        else
            _asuser="su -s /bin/sh -c"
        fi
        $_asuser "mkdir -p '$_home/.local/share/nautilus/scripts' '$_home/.local/share/nautilus-python/extensions'" || :
        $_asuser "ln -sfn '%{_datadir}/pixel-connect/pixel-connect-send' '$_home/.local/share/nautilus/scripts/pixel-connect-send'" || :
        if [ -f %{_datadir}/nautilus-python/extensions/pixel-connect-share.py ]; then
            $_asuser "ln -sfn '%{_datadir}/nautilus-python/extensions/pixel-connect-share.py' '$_home/.local/share/nautilus-python/extensions/pixel-connect-share.py'" || :
        fi
    fi
fi
exit 0

%postun
%{_bindir}/gtk-update-icon-cache -q %{_datadir}/icons/hicolor 2>/dev/null || :
%{_bindir}/update-desktop-database -q %{_datadir}/applications 2>/dev/null || :

if [ "$1" = "0" ] && [ -n "$USER" ] && [ "$USER" != "root" ]; then
    _home="$(getent passwd "$USER" | cut -d: -f6)"
    if [ -n "$_home" ]; then
        rm -f "$_home/.local/share/nautilus/scripts/pixel-connect-send"
        rm -f "$_home/.local/share/nautilus-python/extensions/pixel-connect-share.py"
    fi
fi
exit 0

%files
%license LICENSE
%doc README.md NOTICE.md
%license %{_datadir}/licenses/pixel-connect/LICENSE
%license %{_datadir}/licenses/pixel-connect/NOTICE.md
%doc %{_datadir}/doc/pixel-connect/README.md
%{_bindir}/pixel-connect
%{_datadir}/pixel-connect/
%{_datadir}/applications/dev.pixelconnect.app.desktop
%{_datadir}/icons/hicolor/128x128/apps/pixel-connect.png
%{_datadir}/icons/hicolor/scalable/apps/pixel-connect-symbolic.svg
%{_datadir}/nautilus-python/extensions/

%changelog
* Sat Oct 10 2026 Pixel Connect contributors <dev@pixelconnect.app> - 0.1.0-1
- Initial V1 release.