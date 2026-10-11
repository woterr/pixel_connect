// Pixel Connect — device row state model.
//
// These lock in the honesty rules the Devices view depends on: a discovered
// device is never called connected, a connected device is never called
// unpaired just because the trust store is empty, and every state carries a
// word rather than relying on colour.

import { describe, expect, it } from 'vitest'

import {
	compareDevices,
	deviceRow,
	deviceStatus,
	primaryAction,
	statusLabel,
	transportLabel,
} from './deviceRow'
import type { KcDevice, LinkState } from '../types'

function device(over: Partial<KcDevice> = {}): KcDevice {
	return {
		id: 'dev-1',
		name: 'Pixel 8',
		type: 'phone',
		protocolVersion: 8,
		pairState: 'NotPaired',
		linkState: 'Disconnected',
		transport: 'lan',
		address: '192.168.1.20',
		incomingCapabilities: [],
		outgoingCapabilities: [],
		features: ['ping'],
		trusted: false,
		verificationKey: null,
		protocolError: null,
		lastSeen: 0,
		batteryLevel: null,
		batteryIsCharging: null,
		...over,
	}
}

const link = (l: LinkState, over: Partial<KcDevice> = {}) =>
	device({ linkState: l, ...over })

describe('deviceStatus', () => {
	it('separates discovered from paired-but-unreachable', () => {
		expect(deviceStatus(link('Disconnected', { trusted: false }))).toBe('discovered')
		expect(deviceStatus(link('Disconnected', { trusted: true }))).toBe('unreachable')
	})

	it('never calls a merely-discovered device connected', () => {
		// A peer that is visible on the network is not usable until linked.
		const seen = link('Disconnected', { trusted: false })
		expect(deviceStatus(seen)).not.toBe('connected')
		expect(statusLabel(seen)).toBe('Discovered')
	})

	it('reports Connected even when the trust store is empty', () => {
		// The phone is paired with a *different* KDE Connect build, so it
		// tolerates this link while nothing is pinned here. Transfers work.
		const d = link('Connected', { trusted: false })
		expect(deviceStatus(d)).toBe('connected')
		expect(statusLabel(d)).toBe('Connected')
	})

	it('distinguishes the two directions of a pairing handshake', () => {
		expect(deviceStatus(link('PairingRequestedByPeer'))).toBe('pairingByPeer')
		expect(deviceStatus(link('PairingRequested'))).toBe('pairing')
	})

	it('treats an errored pending pairing as failed, not pending', () => {
		const d = link('PairingRequested', { protocolError: 'handshake refused' })
		expect(deviceStatus(d)).toBe('pairingFailed')
		// and the row offers a retry rather than sitting on "Pairing…"
		expect(primaryAction(d, false).id).toBe('pair')
	})

	it('keeps a stale error from overriding a live link', () => {
		const d = link('Connected', { trusted: true, protocolError: 'old failure' })
		expect(deviceStatus(d)).toBe('connected')
	})

	it('reports reconnecting while a dial is in progress', () => {
		expect(deviceStatus(link('Connecting', { trusted: true }))).toBe('reconnecting')
	})
})

describe('statusLabel', () => {
	it('always yields a word, never an empty label', () => {
		const states: LinkState[] = [
			'Unpaired',
			'Disconnected',
			'PairingRequested',
			'PairingRequestedByPeer',
			'Connecting',
			'Connected',
		]
		for (const l of states) {
			expect(statusLabel(link(l, { trusted: l !== 'Unpaired' })).length).toBeGreaterThan(0)
		}
	})
})

describe('primaryAction', () => {
	it('blocks a duplicate Pair click while one is already in flight', () => {
		const d = link('Disconnected')
		expect(primaryAction(d, false)).toMatchObject({ id: 'pair', busy: false })
		expect(primaryAction(d, true)).toMatchObject({ id: 'pair', busy: true })
	})

	it('does not offer a busy action for a settled state', () => {
		// A connected device is pingable even though a ping is in flight.
		expect(primaryAction(link('Connected'), false)).toMatchObject({
			id: 'ping',
			busy: false,
		})
	})

	it('offers the confirming action when the phone asks to pair', () => {
		expect(primaryAction(link('PairingRequestedByPeer'), false).id).toBe('accept')
	})

	it('offers a retry rather than a repeat pairing for a trusted peer', () => {
		expect(primaryAction(link('Disconnected', { trusted: true }), false).id).toBe('retry')
	})
})

describe('transportLabel', () => {
	it('names the transport in words rather than a protocol constant', () => {
		expect(transportLabel('lan')).toBe('Wi-Fi')
		expect(transportLabel('bluetooth')).toBe('Bluetooth')
		expect(transportLabel('wifiDirect')).toBe('Wi-Fi Direct')
	})

	it('reports no transport rather than inventing one', () => {
		expect(transportLabel('none')).toBeNull()
		expect(transportLabel(null)).toBeNull()
	})
})

describe('deviceRow', () => {
	it('omits battery until the phone actually reports it', () => {
		const row = deviceRow(device(), { busy: false, preferredId: null, defaultId: null })
		expect(row.detail).not.toContain('%')
	})

	it('shows battery once it is known', () => {
		const row = deviceRow(device({ batteryLevel: 82, batteryIsCharging: false }), {
			busy: false,
			preferredId: null,
			defaultId: null,
		})
		expect(row.detail).toContain('82%')
	})

	it('names the transport in the connected line', () => {
		const row = deviceRow(device({ linkState: 'Connected', trusted: true }), {
			busy: false,
			preferredId: null,
			defaultId: null,
		})
		expect(row.detail).toContain('Connected over Wi-Fi')
		expect(row.transportLabel).toBe('Wi-Fi')
	})

	it('allows unpairing only for a paired device', () => {
		const base = { busy: false, preferredId: null, defaultId: null }
		expect(deviceRow(device(), base).canUnpair).toBe(false)
		expect(deviceRow(device({ trusted: true }), base).canUnpair).toBe(true)
	})
})

describe('compareDevices', () => {
	it('puts what needs attention first and connected last', () => {
		const needsYou = device({ id: 'a', name: 'needs you', linkState: 'PairingRequestedByPeer' })
		const live = device({ id: 'b', name: 'live', linkState: 'Connected', trusted: true })
		const seen = device({ id: 'c', name: 'seen' })
		expect([live, seen, needsYou].sort(compareDevices).map((d) => d.id)).toEqual([
			'a',
			'c',
			'b',
		])
	})
})
