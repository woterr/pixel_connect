#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-only OR GPL-3.0-only
#
# Pixel Connect — Nautilus "Send to Pixel" context-menu extension.
#
# Adapted from KDE Connect's Nautilus extension:
#   https://github.com/KDE/kdeconnect-kde/tree/master/nautilus-extension
#   (nautilus-extension/kdeconnect-share.py)
#   Copyright (C) 2011-2016 Aleksey Filippov, and the KDE Connect contributors.
#   Licensed GPL-2.0-only OR GPL-3.0-only OR LicenseRef-KDE-Accepted-GPL.
#
# Pixel Connect is *not* KDE Connect: it does not talk to `org.kde.kdeconnect`
# and does not require kdeconnectd to be installed. It talks to the running
# Pixel Connect process over the user's runtime directory socket, using the same
# JSON request format the `pixel-connect-send` script uses:
#
#     {"paths": ["file:///…"], "deviceId": null, "reveal": true}
#
# installed as:
#     ~/.local/share/nautilus-python/extensions/pixel-connect-share.py
#
# GNOME Files loads extensions from that directory when the `nautilus-python`
# package is installed. On systems where Python extensions are unavailable,
# `pixel-connect-send` is installed as a Nautilus *script* instead and the same
# backend is used.

import json
import os
import socket
import subprocess
import urllib.parse

import gi

# Nautilus has bumped this namespace more than once and distros ship different
# typelibs: Fedora 44 provides `Nautilus-4.1.typelib` only, so requiring "4.0"
# outright makes the extension fail to import and the context-menu entry never
# appears at all. Try the newest first and fall back.
_NAUTILUS_VERSION = None
for _candidate in ("4.1", "4.0", "3.0"):
    try:
        gi.require_version("Nautilus", _candidate)
        _NAUTILUS_VERSION = _candidate
        break
    except ValueError:
        continue
if _NAUTILUS_VERSION is None:
    raise ImportError("no usable Nautilus typelib found (tried 4.1, 4.0, 3.0)")
from gi.repository import GObject, Nautilus  # noqa: E402

# XDG_RUNTIME_DIR is the authoritative location; fall back to /tmp so the
# extension never crashes on an unusual session.
RUNTIME_DIR = os.environ.get("XDG_RUNTIME_DIR") or "/tmp"
SOCKET_PATH = os.path.join(RUNTIME_DIR, "pixel-connect", "filemanager.sock")
LABEL = "Send to Pixel"


def _notify(parent, kind, message):
    """Report the outcome as a desktop notification.

    This deliberately does not use Gtk. Nautilus extensions run inside the
    GTK 4 process, and importing Gtk 3 there is a well known way to abort the
    whole file manager - the failure mode being Files simply refusing to open.
    `notify-send` keeps the message on screen and leaves the toolkit alone;
    if even that is missing we fall back to Nautilus's own information bar.
    """
    urgency = "critical" if kind == "ERROR" else "normal"
    try:
        subprocess.Popen(
            ["notify-send", "-a", "Pixel Connect", "-u", urgency, "Send to Pixel", message],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        return
    except Exception:
        pass
    # Every fallback here has to tolerate `parent` being None or lacking the
    # method: this runs inside Nautilus's process, where an exception escaping
    # is logged as an unhandled error and the user simply never learns whether
    # the send worked.
    try:
        getter = getattr(parent, "get_window", None)
        window = getter() if callable(getter) else None
        if window is not None:
            window.emit("message", "Pixel Connect", message)
            return
    except Exception:
        pass
    print(f"Pixel Connect: {message}")


def backend_running():
    return os.path.exists(SOCKET_PATH)


def send(paths, device_id=None):
    """Send a list of URIs to the running Pixel Connect backend."""
    if not backend_running():
        return False, (
            "Pixel Connect is not running.\n"
            "Start it, then try “Send to Pixel” again."
        )

    request = json.dumps(
        {"paths": list(paths), "deviceId": device_id, "reveal": True}
    ).encode("utf-8")

    try:
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as sock:
            sock.settimeout(20)
            sock.connect(SOCKET_PATH)
            sock.sendall(request + b"\n")
            chunks = []
            while True:
                chunk = sock.recv(4096)
                if not chunk:
                    break
                chunks.append(chunk)
                if b"\n" in chunk:
                    break
    except OSError as exc:
        return False, f"Could not reach Pixel Connect: {exc}"

    if not chunks:
        return False, "Pixel Connect did not answer."

    try:
        response = json.loads(chunks[0].decode("utf-8").strip())
    except ValueError:
        return False, "Pixel Connect sent a malformed reply."

    if not response.get("ok"):
        return False, response.get("error") or "The transfer could not be started."

    sent = response.get("sent", 0)
    device = response.get("device")
    noun = "item" if sent == 1 else "items"
    where = f" to {device}" if device else ""
    return True, f"Sent {sent} {noun}{where}."


class PixelConnectMenuProvider(GObject.GObject, Nautilus.MenuProvider):
    """Adds "Send to Pixel" to the Nautilus file context menu."""

    # Nautilus instantiates the class found in this module. The type name has to
    # be declared explicitly: the previous `GObject.type_register()` call
    # registered the type under a derived name, and instantiating the class then
    # raised `SystemError: returned a result with an exception set`, so Nautilus
    # silently dropped the extension and no context-menu entry ever appeared.
    __gtype_name__ = "PixelConnectMenuProvider"

    def _eligible(self, files):
        """Only offer the action for local, existing, non-hidden files.

        Nautilus 4.x passes a single `Nautilus.FileInfo` to
        `get_background_items` but a list to `get_file_items`, so normalise
        both shapes here. Treating the single object as a list raised
        `TypeError: 'NautilusVFSFile' object is not iterable` on every menu
        build.
        """
        if files is None:
            return False
        if hasattr(files, "get_uri_scheme"):
            files = [files]
        for f in files:
            if f.get_uri_scheme() != "file":
                return False
            if f.is_directory():
                continue
            path = urllib.parse.unquote(f.get_uri()[7:])
            if not path or path.endswith("~"):
                return False
        return bool(files)

    def _background_items(self, name, files):
        item = Nautilus.MenuItem(
            name=name,
            label=LABEL,
            tip="Send the selected items to your Pixel over the local network",
        )
        item.connect("activate", self._on_activate, files)
        return item

    def get_background_items(self, current_folder):
        """Nautilus 4.x signature: a single FileInfo for the open folder."""
        return [self._background_items("PixelConnect::send_bg", [])]

    def get_file_items(self, files):
        if not self._eligible(files):
            return []
        return [self._background_items("PixelConnect::send_files", files)]

    def get_item_for_file(self, file):
        """Called once per file by Nautilus 4.x.

        This is the method Nautilus actually uses on current versions;
        implementing only `get_file_items` leaves no entry in the menu at all.
        """
        if not self._eligible([file]):
            return None
        return self._background_items("PixelConnect::send_one", [file])

    def _on_activate(self, _item, files):
        uris = []
        for f in files:
            uri = f.get_uri()
            # A directory is sent as a flattened set of files, matching KDE
            # Connect's own behaviour.
            uris.append(uri)
        ok, message = send(uris)
        # Pass `self`, not `self.get_window()`. MenuProvider has no
        # `get_window` method, so calling it here raised
        #
        #     AttributeError: 'PixelConnectMenuProvider' object has no
        #     attribute 'get_window'
        #
        # on every activation, and the user got no notification at all - the
        # send silently appeared to do nothing. `_notify` resolves the window
        # itself and tolerates its absence.
        _notify(self, "INFO" if ok else "ERROR", message)

# No explicit `GObject.type_register()` here.
#
# `__gtype_name__` above already registers the type, and calling
# `type_register()` a second time re-registers the same name. GLib then
# aborts with
#
#     g_boxed_type_register_static: assertion
#     'g_type_from_name (name) == 0' failed
#
# which crashed Nautilus outright instead of merely hiding the menu entry.
# PyGObject registers the class automatically and Nautilus discovers the
# MenuProvider subclass from this module.
