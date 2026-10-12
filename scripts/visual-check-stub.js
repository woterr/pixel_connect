// Pixel Connect — visual verification harness (development tool, NOT shipped)
//
// This file is loaded *instead of* the real Tauri IPC layer so the production
// front end bundle can be rendered in a plain browser for screenshots and
// layout checks. It answers the same `invoke` commands the Rust backend does,
// with values that match the Rust types exactly.
//
// It is never referenced by index.html or by any component; `scripts/visual-check.sh`
// copies it in only for the duration of a screenshot run.

globalThis.__TAURI_INTERNALS__ = {
	invoke(cmd, args) {
		const now = Math.floor(Date.now() / 1000)
		switch (cmd) {
			case 'app_info':
				return {
					name: 'pixel-connect',
					version: '0.1.0',
					identifier: 'dev.pixelconnect.app',
					kdeconnectProtocol: 8,
					quickshareService: true,
					clipboardBackend: 'Wayland',
					filemanagerSocket: '/run/user/1000/pixel-connect/filemanager.sock',
				}
			case 'get_settings':
				return {
					realclose: false,
					startminimized: false,
					autostart: false,
					// The stub must mirror what the backend actually returns.
					// Omitting this made the theme picker render with nothing
					// selected on load, which looked like a bug in the picker but
					// was the harness disagreeing with the real API.
					theme: 'dark',
					visibility: 0,
					port: null,
					downloadPath: '/home/user/Downloads',
					incomingPath: '/home/user/Downloads',
					debugLevel: null,
					clipboardEnabled: true,
					clipboardAutoShare: true,
					kdeDeviceName: 'fedora',
					nautilusEnabled: true,
					preferredDeviceId: 'a'.repeat(32),
				}
			case 'update_settings':
				return args.settings
			case 'clipboard_capabilities':
				return {
					backend: 'Wayland',
					wayland: true,
					x11: false,
					backgroundReadAllowed: false,
					note: 'GNOME only lets a focused window read the clipboard.',
				}
			case 'adb_detect':
				return {
					adbPath: '/usr/bin/adb',
					scrcpyPath: '/usr/bin/scrcpy',
					adbVersion: 'Android Debug Bridge version 1.0.41',
					scrcpyVersion: 'scrcpy 4.1',
					error: null,
				}
			case 'adb_devices':
				return [
					{
						serial: 'R5CT30XXXXX',
						state: 'device',
						kind: 'usb',
						model: 'Pixel 8 Pro',
						product: 'raven',
						device: 'raven',
						transportId: '2',
						label: 'Pixel 8 Pro',
						scrcpyReady: true,
						scrcpyBlockReason: null,
					},
					{
						serial: 'DEADBEEF',
						state: 'unauthorized',
						kind: 'usb',
						model: null,
						product: null,
						device: null,
						transportId: null,
						label: 'DEADBEEF',
						scrcpyReady: false,
						scrcpyBlockReason:
							'The phone has not accepted this computer for debugging.',
					},
				]
			case 'kde_devices':
				return [
					{
						id: 'a'.repeat(32),
						name: "Maria's Pixel",
						type: 'phone',
						protocolVersion: 8,
						pairState: 'Paired',
						linkState: 'Connected',
						transport: 'LAN',
						address: '192.168.1.42:1717',
						incomingCapabilities: [
							'kdeconnect.clipboard',
							'kdeconnect.clipboard.connect',
							'kdeconnect.share.request',
							'kdeconnect.battery',
						],
						outgoingCapabilities: [
							'kdeconnect.clipboard',
							'kdeconnect.share.request',
							'kdeconnect.ping',
						],
						features: [
							'clipboard',
							'receiveFiles',
							'shareFiles',
							'ping',
							'battery',
						],
						trusted: true,
						verificationKey: null,
						protocolError: null,
						lastSeen: now - 30,
						batteryLevel: 76,
						batteryIsCharging: false,
					},
					{
						id: 'b'.repeat(32),
						name: 'Pixel Tablet',
						type: 'tablet',
						protocolVersion: 8,
						pairState: 'NotPaired',
						linkState: 'PairingRequestedByPeer',
						transport: 'LAN',
						address: '192.168.1.51:1718',
						incomingCapabilities: [],
						outgoingCapabilities: ['kdeconnect.clipboard'],
						features: [],
						trusted: false,
						verificationKey: 'A3F91C42',
						protocolError: null,
						lastSeen: now - 8,
						batteryLevel: null,
						batteryIsCharging: null,
					},
				]
			case 'kde_pending_shares':
				return []
			case 'kde_notifications':
				return [
					{
						key: "a'.repeat(32)",
						title: 'Architecture_Specs.pdf',
						text: 'Downloaded to ~/Downloads',
						appName: 'Files',
						timestamp: now - 120,
						actions: [],
						isClearable: true,
					},
				]
			case 'filemanager_status':
				return {
					socket: '/run/user/1000/pixel-connect/filemanager.sock',
					listening: true,
					pythonExtensionInstalled: true,
					scriptInstalled: false,
					note: 'The Nautilus context-menu entry "Send to Pixel" is installed.',
				}
			case 'scrcpy_running':
				return []
			case 'ssh_available':
				return { available: true, path: '/usr/bin/ssh' }
			case 'quickshare_start_discovery':
			case 'quickshare_stop_discovery':
			case 'kde_refresh':
				return null
			default:
				return null
		}
	},
	convertFileSrc: (p) => p,
	metadata: { currentWindow: { label: 'main' }, currentWebview: { window: { label: 'main' } } },
	plugins: {},
	transformCallback: (cb) => cb,
}

window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
	invoke: () => Promise.resolve(null),
	unregisterListener: () => Promise.resolve(),
}