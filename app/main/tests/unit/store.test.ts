// Pixel Connect — store logic.
//
// The store is the single source of truth for what the UI shows, so its derived
// values (which device is "preferred", what counts as "online") are pinned down
// here. No component behaviour is mocked: the store is exercised directly.

import { beforeEach, describe, expect, it } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

import { useAppStore } from '../../src/stores/app'
import type { AdbDevice, KcDevice } from '../../src/types'

function device(overrides: Partial<KcDevice> = {}): KcDevice {
	return {
		id: 'a'.repeat(32),
		name: 'Pixel',
		type: 'phone',
		protocolVersion: 8,
		pairState: 'Paired',
		linkState: 'Connected',
		transport: 'lan',
		address: '192.168.1.42:1717',
		incomingCapabilities: ['kdeconnect.clipboard'],
		outgoingCapabilities: ['kdeconnect.share.request'],
		features: ['clipboard', 'receiveFiles', 'shareFiles'],
		trusted: true,
		verificationKey: null,
		protocolError: null,
		lastSeen: 0,
		batteryLevel: null,
		batteryIsCharging: null,
		...overrides,
	}
}

describe('device store', () => {
	beforeEach(() => setActivePinia(createPinia()))

	it('separates connected from merely paired devices', () => {
		const store = useAppStore()
		store.devices = [
			device({ id: 'a'.repeat(32), linkState: 'Connected' }),
			device({ id: 'b'.repeat(32), linkState: 'Disconnected' }),
			device({ id: 'c'.repeat(32), linkState: 'Connected', trusted: false }),
		]
		expect(store.connectedDevices.map((d: KcDevice) => d.id)).toEqual([
			'a'.repeat(32),
			'c'.repeat(32),
		])
		// "Paired" means trusted, regardless of the link state.
		expect(store.pairedDevices.map((d: KcDevice) => d.id)).toEqual([
			'a'.repeat(32),
			'b'.repeat(32),
		])
	})

	it('prefers a connected device, then any paired device, then null', () => {
		const store = useAppStore()
		expect(store.preferredDevice).toBeNull()

		store.devices = [device({ id: 'b'.repeat(32), linkState: 'Disconnected' })]
		expect(store.preferredDevice?.id).toBe('b'.repeat(32))

		store.devices = [
			device({ id: 'b'.repeat(32), linkState: 'Disconnected' }),
			device({ id: 'a'.repeat(32), linkState: 'Connected' }),
		]
		expect(store.preferredDevice?.id).toBe('a'.repeat(32))
	})

	it('honours an explicit preferred device even when disconnected', () => {
		const store = useAppStore()
		store.settings = {
			preferredDeviceId: 'b'.repeat(32),
		} as never
		store.devices = [
			device({ id: 'a'.repeat(32), linkState: 'Connected' }),
			device({ id: 'b'.repeat(32), linkState: 'Disconnected' }),
		]
		expect(store.preferredDevice?.id).toBe('b'.repeat(32))
	})

	it('reports online only when something is actually reachable', () => {
		const store = useAppStore()
		expect(store.isOnline).toBe(false)

		store.devices = [device({ linkState: 'Disconnected' })]
		expect(store.isOnline).toBe(false)

		store.quickshareEndpoints = [
			{ name: 'Pixel', device_type: 'phone', address: null, port: null },
		]
		expect(store.isOnline).toBe(true)
	})

	it('splits ADB devices by connection kind', () => {
		const store = useAppStore()
		store.adbDevices = [
			{ serial: 'USB1', kind: 'usb' } as never,
			{ serial: 'WIFI1', kind: 'wireless' } as never,
			{ serial: 'emu', kind: 'emulator' } as never,
		]
		expect(store.usbDevices.map((d: AdbDevice) => d.serial)).toEqual(['USB1'])
		expect(store.wirelessDevices.map((d: AdbDevice) => d.serial)).toEqual(['WIFI1'])
	})

	it('only offers scrcpy for devices the backend marked ready', () => {
		const store = useAppStore()
		store.adbDevices = [
			{ serial: 'OK', scrcpyReady: true } as never,
			{ serial: 'BAD', scrcpyReady: false } as never,
		]
		expect(store.scrcpyReadyDevices.map((d: AdbDevice) => d.serial)).toEqual(['OK'])
	})

	it('upserts devices instead of duplicating them', () => {
		const store = useAppStore()
		store.upsertDevice(device({ name: 'Old name' }))
		store.upsertDevice(device({ name: 'New name' }))
		expect(store.devices).toHaveLength(1)
		expect(store.devices[0].name).toBe('New name')
	})

	it('derives Quick Share progress from acknowledged bytes', () => {
		const store = useAppStore()
		store.quickshareMeta = {
			id: 'x',
			source: null,
			pin_code: null,
			destination: null,
			files: null,
			text_type: null,
			text_description: null,
			text_payload: null,
			total_bytes: 1000,
			ack_bytes: 250,
		} as never
		expect(store.quickShareProgress).toBe(0.25)
		expect(store.quickShareReceiving).toBe(false)

		store.quickshareState = 'ReceivingFiles'
		expect(store.quickShareReceiving).toBe(true)
	})

	it('treats a zero-byte payload as no progress rather than dividing by zero', () => {
		const store = useAppStore()
		store.quickshareMeta = {
			total_bytes: 0,
			ack_bytes: 0,
		} as never
		expect(store.quickShareProgress).toBe(0)
	})

	it('replaces a transfer in place rather than appending duplicates', () => {
		const store = useAppStore()
		store.applyTransfer({
			id: 'x',
			deviceId: 'a',
			filename: 'a.zip',
			bytes: 10,
			total: 100,
			done: false,
			error: null,
		})
		store.applyTransfer({
			id: 'x',
			deviceId: 'a',
			filename: 'a.zip',
			bytes: 90,
			total: 100,
			done: false,
			error: null,
		})
		expect(store.transfers).toHaveLength(1)
		expect(store.transfers[0].bytes).toBe(90)
		expect(store.activeTransfers).toHaveLength(1)
	})

	it('clears the Quick Share payload when a transfer ends', () => {
		const store = useAppStore()
		store.quickshareMeta = { total_bytes: 10, ack_bytes: 10 } as never
		store.applyQuickShareEvent({ state: 'Finished' } as never)
		expect(store.quickshareMeta).toBeNull()
	})

	it('expires toasts', async () => {
		const store = useAppStore()
		store.toast('hello')
		expect(store.toasts).toHaveLength(1)
		store.dismissToast(store.toasts[0].id)
		expect(store.toasts).toHaveLength(0)
	})
})