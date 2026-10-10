// Pixel Connect — typed wrappers around the Tauri command surface.
//
// Every UI action goes through this module, so components never talk to the
// IPC layer directly and the backend contract stays in one place.

import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

import type {
	AdbDevice,
	AppInfo,
	ChannelMessage,
	ClipboardCapabilities,
	DeviceInfo,
	EndpointInfo,
	FileManagerStatus,
	KcDevice,
	PendingShare,
	RemoteNotification,
	ScrcpyLaunch,
	SendInfo,
	Settings,
	SshProbe,
	SshTarget,
	ToolAvailability,
	TransferProgress,
	Visibility,
} from './types'

// ------------------------------------------------------------------ app

export const appInfo = () => invoke<AppInfo>('app_info')

export const getSettings = () => invoke<Settings>('get_settings')

export const updateSettings = (settings: Settings) =>
	invoke<Settings>('update_settings', { settings })

export const openHelp = () => invoke<void>('open_help')

export const quitApp = () => invoke<void>('quit_app')

export const getHostname = () => invoke<string>('get_hostname')

// ---------------------------------------------------------- Quick Share

export const quickshareStartDiscovery = () =>
	invoke<void>('quickshare_start_discovery')

export const quickshareStopDiscovery = () =>
	invoke<void>('quickshare_stop_discovery')

export const quickshareSendPayload = (payload: SendInfo) =>
	invoke<void>('quickshare_send_payload', { payload })

export const quickshareSetVisibility = (visibility: Visibility) =>
	invoke<void>('quickshare_set_visibility', { visibility })

export const quickshareSetDownloadPath = (path: string | null) =>
	invoke<void>('quickshare_set_download_path', { path })

export const quickshareAcceptTransfer = (id: string) =>
	invoke<void>('quickshare_accept_transfer', { id })

export const quickshareRejectTransfer = (id: string) =>
	invoke<void>('quickshare_reject_transfer', { id })

// -------------------------------------------------------- KDE Connect

export const kdeDevices = () => invoke<KcDevice[]>('kde_devices')

export const kdeDeviceInfo = () => invoke<DeviceInfo>('kde_device_info')

export const kdeRefresh = () => invoke<void>('kde_refresh')

export const kdeRequestPairing = (deviceId: string) =>
	invoke<void>('kde_request_pairing', { deviceId })

export const kdeAcceptPairing = (deviceId: string) =>
	invoke<void>('kde_accept_pairing', { deviceId })

export const kdeRejectPairing = (deviceId: string) =>
	invoke<void>('kde_reject_pairing', { deviceId })

export const kdeUnpair = (deviceId: string) =>
	invoke<boolean>('kde_unpair', { deviceId })

export const kdePing = (deviceId: string, message: string) =>
	invoke<void>('kde_ping', { deviceId, message })

export const kdeShareText = (deviceId: string, text: string) =>
	invoke<void>('kde_share_text', { deviceId, text })

export const kdeShareFiles = (deviceId: string, paths: string[]) =>
	invoke<number>('kde_share_files', { deviceId, paths })

export const kdeShareDirectory = (deviceId: string, path: string) =>
	invoke<number>('kde_share_directory', { deviceId, path })

export const kdePendingShares = () => invoke<PendingShare[]>('kde_pending_shares')

export const kdeDismissShare = (id: string) => invoke<void>('kde_dismiss_share', { id })

export const kdeNotifications = () =>
	invoke<RemoteNotification[]>('kde_notifications')

export const kdeClearNotifications = () => invoke<void>('kde_clear_notifications')

export const kdeSendNotificationAction = (deviceId: string, key: string, action: string) =>
	invoke<void>('kde_send_notification_action', { deviceId, key, action })

export const kdeRequestClipboard = () => invoke<number>('kde_request_clipboard')

export const kdeSetClipboardEnabled = (enabled: boolean) =>
	invoke<void>('kde_set_clipboard_enabled', { enabled })

export const kdeSetClipboardAutoShare = (enabled: boolean) =>
	invoke<void>('kde_set_clipboard_auto_share', { enabled })

export const kdeSetDeviceName = (name: string) =>
	invoke<void>('kde_set_device_name', { name })

export const kdeSetIncomingPath = (path: string) =>
	invoke<void>('kde_set_incoming_path', { path })

// ------------------------------------------------------------ clipboard

export const clipboardRead = () => invoke<string | null>('clipboard_read')

export const clipboardWrite = (text: string) => invoke<void>('clipboard_write', { text })

export const clipboardCapabilities = () =>
	invoke<ClipboardCapabilities>('clipboard_capabilities')

// ------------------------------------------------------------- ADB/scrcpy

export const adbDetect = () => invoke<ToolAvailability>('adb_detect')

export const adbDevices = () => invoke<AdbDevice[]>('adb_devices')

export const scrcpyLaunch = (serial: string, args: string[] = []) =>
	invoke<ScrcpyLaunch>('scrcpy_launch', { serial, args })

export const scrcpyStop = (serial: string) => invoke<boolean>('scrcpy_stop', { serial })

export const scrcpyRunning = () => invoke<string[]>('scrcpy_running')

// ------------------------------------------------------------------ SSH

export const sshAvailable = () =>
	invoke<{ available: boolean; path: string | null }>('ssh_available')

export const sshProbe = (target: SshTarget) => invoke<SshProbe>('ssh_probe', { target })

export const sshOpen = (target: SshTarget, terminal?: string) =>
	invoke<string>('ssh_open', { target, terminal })

// --------------------------------------------------------- file manager

export const filemanagerStatus = () => invoke<FileManagerStatus>('filemanager_status')

export const filemanagerSendPaths = (paths: string[], deviceId?: string) =>
	invoke<{ ok: boolean; sent: number; device: string | null; error: string | null }>(
		'filemanager_send_paths',
		{ paths, deviceId },
	)

// ---------------------------------------------------------------- events

export interface BackendEvents {
	'kdeconnect:device': KcDevice
	'kdeconnect:removed': string
	'kdeconnect:pairing_request': KcDevice
	'kdeconnect:clipboard': { deviceId: string; deviceName: string }
	'kdeconnect:share': PendingShare
	'kdeconnect:notification': RemoteNotification
	'kdeconnect:connectivity': {
		deviceId: string
		wifiStrength: number | null
		wifiName: string | null
		bluetoothStrength: number | null
		cellularStrength: number | null
		cellularName: string | null
	}
	'kdeconnect:battery': { deviceId: string; level: number; charging: boolean }
	'kdeconnect:transfer': TransferProgress
	'quickshare:event': ChannelMessage
	'quickshare:endpoint': EndpointInfo
}

/** Subscribe to a backend event. Returns an unsubscribe function. */
export function on<K extends keyof BackendEvents>(
	event: K,
	handler: (payload: BackendEvents[K]) => void,
): Promise<UnlistenFn> {
	return listen<BackendEvents[K]>(event, (e) => handler(e.payload))
}