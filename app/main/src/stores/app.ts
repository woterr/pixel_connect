// Pixel Connect — shared application state.
//
// One store owns the backend truth: devices, transfers, shares, notifications,
// ADB devices and settings. Components read from here and call the API module;
// nothing is mocked.

import { defineStore } from 'pinia'
import { computed, ref } from 'vue'

import * as api from '../api'
import type {
	AdbDevice,
	AppInfo,
	ChannelMessage,
	ClipboardCapabilities,
	FileManagerStatus,
	KcDevice,
	PendingShare,
	RemoteNotification,
	Settings,
	ToolAvailability,
	TransferProgress,
	TransportStatus,
} from '../types'

export interface Toast {
	id: number
	kind: 'info' | 'success' | 'error'
	message: string
}

export const useAppStore = defineStore('app', () => {
	// ---- backend state ------------------------------------------------
	const info = ref<AppInfo | null>(null)
	const settings = ref<Settings | null>(null)
	const devices = ref<KcDevice[]>([])
	const shares = ref<PendingShare[]>([])
	const notifications = ref<RemoteNotification[]>([])
	const transfers = ref<TransferProgress[]>([])
	const adbDevices = ref<AdbDevice[]>([])
	const tools = ref<ToolAvailability | null>(null)
	const clipboardCaps = ref<ClipboardCapabilities | null>(null)
	const filemanager = ref<FileManagerStatus | null>(null)
	const scrcpySessions = ref<string[]>([])
	const transports = ref<TransportStatus[]>([])

	// ---- Quick Share state -------------------------------------------
	const quickshareState = ref<ChannelMessage['state']>(null)
	const quickshareMeta = ref<ChannelMessage['meta']>(null)
	const quickshareEndpoints = ref<api.BackendEvents['quickshare:endpoint'][]>([])
	const quickshareEnabled = ref(true)

	// ---- transient ----------------------------------------------------
	const toasts = ref<Toast[]>([])
	const pairingRequests = ref<KcDevice[]>([])
	let toastSeq = 0

	function toast(message: string, kind: Toast['kind'] = 'info') {
		const id = ++toastSeq
		toasts.value.push({ id, kind, message })
		setTimeout(() => {
			toasts.value = toasts.value.filter((t) => t.id !== id)
		}, kind === 'error' ? 7000 : 4000)
	}

	function dismissToast(id: number) {
		toasts.value = toasts.value.filter((t) => t.id !== id)
	}

	// ---- derived ------------------------------------------------------
	const connectedDevices = computed(() =>
		devices.value.filter((d) => d.linkState === 'Connected'),
	)

	const pairedDevices = computed(() => devices.value.filter((d) => d.trusted))

	const preferredDevice = computed<KcDevice | null>(() => {
		const wanted = settings.value?.preferredDeviceId
		if (wanted) {
			const hit = devices.value.find((d) => d.id === wanted)
			if (hit) return hit
		}
		return connectedDevices.value[0] ?? pairedDevices.value[0] ?? null
	})

	const activeTransfers = computed(() => transfers.value.filter((t) => !t.done))

	const usbDevices = computed(() =>
		adbDevices.value.filter((d) => d.kind === 'usb'),
	)
	const wirelessDevices = computed(() =>
		adbDevices.value.filter((d) => d.kind === 'wireless'),
	)
	const scrcpyReadyDevices = computed(() =>
		adbDevices.value.filter((d) => d.scrcpyReady),
	)

	/** Is anything using the network right now? Drives the ONLINE pill. */
	const isOnline = computed(
		() =>
			connectedDevices.value.length > 0 ||
			quickshareEndpoints.value.length > 0 ||
			activeTransfers.value.length > 0,
	)

	const quickShareReceiving = computed(() => quickshareState.value === 'ReceivingFiles')
	const quickShareSending = computed(() => quickshareState.value === 'SendingFiles')
	const quickShareAwaitingConsent = computed(
		() => quickshareState.value === 'WaitingForUserConsent',
	)

	/** Overall transfer progress for the big headline, 0..1. */
	const quickShareProgress = computed(() => {
		const meta = quickshareMeta.value
		if (!meta || meta.total_bytes === 0) return 0
		return Math.min(1, meta.ack_bytes / meta.total_bytes)
	})

	const kdeTransferProgress = computed(() => {
		const t = transfers.value.find((x) => !x.done && x.total > 0)
		return t ? Math.min(1, t.bytes / t.total) : null
	})

	// ---- actions ------------------------------------------------------
	async function refreshAll() {
		const [devicesResult, adbResult] = await Promise.allSettled([
			api.kdeDevices(),
			api.adbDevices(),
		])
		if (devicesResult.status === 'fulfilled') devices.value = devicesResult.value
		if (adbResult.status === 'fulfilled') adbDevices.value = adbResult.value
	}

	async function bootstrap() {
		try {
			info.value = await api.appInfo()
			settings.value = await api.getSettings()
			clipboardCaps.value = await api.clipboardCapabilities()
			tools.value = await api.adbDetect()
			filemanager.value = await api.filemanagerStatus()
			devices.value = await api.kdeDevices()
			shares.value = await api.kdePendingShares()
			notifications.value = await api.kdeNotifications()
			adbDevices.value = await api.adbDevices()
			scrcpySessions.value = await api.scrcpyRunning()
			transports.value = await api.kdeTransports()
		} catch (e) {
			toast(`Could not initialise the backend: ${e}`, 'error')
		}
	}

	/** Display name for a device id, or a neutral fallback. */
	function deviceName(id: string, fallback = 'Device'): string {
		return devices.value.find((d) => d.id === id)?.name ?? fallback
	}

	function upsertDevice(device: KcDevice) {
		const i = devices.value.findIndex((d) => d.id === device.id)
		if (i === -1) devices.value.push(device)
		else devices.value[i] = device
	}

	function applyQuickShareEvent(message: ChannelMessage) {
		quickshareState.value = message.state
		if (message.meta) quickshareMeta.value = message.meta
		if (message.state === 'Disconnected' || message.state === 'Finished') {
			quickshareMeta.value = null
		}
	}

	function applyTransfer(t: TransferProgress) {
		const i = transfers.value.findIndex(
			(x) => x.deviceId === t.deviceId && x.filename === t.filename,
		)
		if (i === -1) transfers.value.push(t)
		else transfers.value[i] = t
		if (t.done && t.error) toast(`Transfer failed: ${t.error}`, 'error')
	}

	return {
		// state
		info,
		settings,
		devices,
		shares,
		notifications,
		transfers,
		adbDevices,
		tools,
		clipboardCaps,
		filemanager,
		scrcpySessions,
		transports,
		quickshareState,
		quickshareMeta,
		quickshareEndpoints,
		quickshareEnabled,
		toasts,
		pairingRequests,
		// derived
		connectedDevices,
		pairedDevices,
		preferredDevice,
		activeTransfers,
		usbDevices,
		wirelessDevices,
		scrcpyReadyDevices,
		isOnline,
		quickShareReceiving,
		quickShareSending,
		quickShareAwaitingConsent,
		quickShareProgress,
		kdeTransferProgress,
		// actions
		toast,
		dismissToast,
		bootstrap,
		refreshAll,
		deviceName,
		upsertDevice,
		applyQuickShareEvent,
		applyTransfer,
	}
})