// Pixel Connect — device row state model.
//
// Pure derivation of what a device row should say and offer, from the backend's
// `KcDevice` record and nothing else. Every field it reads is real: the
// pairing state, the link state, the transport, the trust flag, the last error
// and the last-seen timestamp all come from `KdeDevice`.
//
// It lives in its own module, with no Vue and no IPC, for two reasons: the
// Devices view stays declarative, and the mapping from backend state to
// wording can be tested directly rather than only through a rendered row.

import type { DeviceType, KcDevice, Transport } from '../types'
import { deviceTypeLabel } from '../utils'

/**
 * The states a row can be in, in the order a person acts on them.
 *
 * These are deliberately more granular than the backend's `LinkState`, because
 * "discovered" and "paired but unreachable" need different words and different
 * buttons even though the backend reports both as a non-`Connected` link.
 */
export type DeviceStatus =
	/** A pairing request is outstanding from the phone; it needs confirming. */
	| 'pairingByPeer'
	/** This app asked to pair and is waiting for the phone. */
	| 'pairing'
	/** We asked to pair and the link could not be completed. */
	| 'pairingFailed'
	/** The link is up and carrying traffic. */
	| 'connected'
	/** A dial is in progress. */
	| 'reconnecting'
	/** Paired and trusted, but the peer cannot be reached right now. */
	| 'unreachable'
	/** Seen on the network, not paired, no error. */
	| 'discovered'
	/** The backend reported a protocol error for this device. */
	| 'failed'

export type StatusTone = 'online' | 'warn' | 'danger' | 'idle'

/** The single action that is most useful for the row's current state. */
export interface PrimaryAction {
	id: 'pair' | 'accept' | 'reject' | 'ping' | 'retry' | 'view'
	label: string
	/** Renders as the filled button. */
	emphasis: boolean
	/** While true the button shows its busy label and cannot be clicked again. */
	busy: boolean
}

export interface DeviceRowModel {
	id: string
	name: string
	/** Raw device type, for choosing an icon. */
	type: DeviceType
	/** Device type as a word, e.g. "Phone". */
	typeLabel: string
	/** Raw transport, for choosing an icon. */
	transport: Transport
	status: DeviceStatus
	/** Short word for the status pill. Never colour alone. */
	statusLabel: string
	tone: StatusTone
	/** One line explaining the state and what it means for the user. */
	detail: string
	/** Transport as a word, or null when there is no transport. */
	transportLabel: string | null
	primary: PrimaryAction
	/** Whether "make this my default device" applies. */
	canBeDefault: boolean
	isDefault: boolean
	canPing: boolean
	canUnpair: boolean
	canMirror: boolean
	/** True while this device is the one the app would send to. */
	isPreferred: boolean
}

const TRANSPORT_LABELS: Record<Transport, string> = {
	lan: 'Wi-Fi',
	bluetooth: 'Bluetooth',
	wifiDirect: 'Wi-Fi Direct',
	none: 'No transport',
}

/** Human wording for a transport, shared by the row and its detail panel. */
export function transportLabel(t: Transport | null | undefined): string | null {
	if (!t || t === 'none') return null
	return TRANSPORT_LABELS[t] ?? t
}

/**
 * Classify a device.
 *
 * Order matters and is deliberate:
 *
 *  - `Connected` is checked first and unconditionally. A link that is
 *    carrying traffic is connected, whatever the trust store says. Reporting
 *    "not paired" here would send the user hunting for a Pair button while
 *    their transfers work perfectly well.
 *  - A pairing handshake in progress outranks an error, because the error is
 *    usually the reason the handshake has not finished yet.
 *  - `trusted` is what separates "seen it, never paired" from "paired with
 *    it, cannot reach it now". Both report a dead link, and they need
 *    opposite actions.
 */
export function deviceStatus(d: KcDevice): DeviceStatus {
	if (d.linkState === 'Connected') return 'connected'
	if (d.linkState === 'PairingRequestedByPeer') return 'pairingByPeer'
	// A pairing request we sent that the backend has since reported an error
	// against is a *failed* pairing, not a pending one. Without this the row
	// would sit on "Pairing…" forever with no way to retry.
	if (d.linkState === 'PairingRequested') {
		return d.protocolError ? 'pairingFailed' : 'pairing'
	}
	if (d.linkState === 'Connecting') return 'reconnecting'
	if (d.protocolError) return 'failed'
	if (d.trusted) return 'unreachable'
	return 'discovered'
}

/** Short, honest wording for the status pill. */
export function statusLabel(d: KcDevice): string {
	switch (deviceStatus(d)) {
		case 'connected':
			return 'Connected'
		case 'pairingByPeer':
			return 'Pairing requested'
		case 'pairing':
			return 'Pairing…'
		case 'pairingFailed':
			return 'Pairing failed'
		case 'reconnecting':
			return 'Reconnecting…'
		case 'unreachable':
			return 'Not reachable'
		case 'failed':
			return 'Connection failed'
		default:
			return 'Discovered'
	}
}

/** Colour band for the status. Always paired with the label above. */
export function statusTone(d: KcDevice): StatusTone {
	switch (deviceStatus(d)) {
		case 'connected':
			return 'online'
		case 'pairingByPeer':
		case 'pairing':
		case 'reconnecting':
			return 'warn'
		case 'failed':
			return 'danger'
		default:
			return 'idle'
	}
}

/**
 * The line under the name.
 *
 * Connection state leads because that is what someone acts on. Battery is
 * appended only when the phone has actually reported it: the field is absent
 * until a battery packet arrives, and a bare "0%" reads as a dead phone.
 *
 * The device type is prefixed for an unpaired device — it is the only
 * information there is before pairing — and dropped once paired, where the
 * name carries that weight already.
 */
export function statusDetail(d: KcDevice, includeType: boolean): string {
	const status = deviceStatus(d)
	const via = transportLabel(d.transport)
	const link =
		status === 'connected' && via ? `Connected over ${via}` : statusLabel(d)

	const parts: string[] = []
	if (includeType) parts.push(deviceTypeLabel(d.type))
	parts.push(link)
	if (d.batteryLevel !== null && d.batteryLevel !== undefined) {
		parts.push(`${d.batteryLevel}%${d.batteryIsCharging ? ' charging' : ''}`)
	}
	return parts.join(' · ')
}

/**
 * The one action worth offering, for the state the device is actually in.
 *
 * `busy` is true whenever the action has already been started, which is what
 * stops a second Pair click from firing a duplicate request while the first is
 * still in flight.
 */
export function primaryAction(d: KcDevice, busy: boolean): PrimaryAction {
	switch (deviceStatus(d)) {
		case 'connected':
			return { id: 'ping', label: 'Ping', emphasis: false, busy: false }
		case 'pairingByPeer':
			return { id: 'accept', label: 'Accept', emphasis: true, busy }
		case 'pairing':
			return { id: 'pair', label: 'Pairing…', emphasis: false, busy: true }
		case 'pairingFailed':
			return { id: 'pair', label: 'Pair again', emphasis: true, busy }
		case 'reconnecting':
			return { id: 'retry', label: 'Reconnecting…', emphasis: false, busy: true }
		case 'failed':
			return { id: 'retry', label: 'Try again', emphasis: true, busy }
		case 'unreachable':
			return { id: 'retry', label: 'Reconnect', emphasis: true, busy }
		default:
			return { id: 'pair', label: 'Pair', emphasis: true, busy }
	}
}

/**
 * Everything a row shows.
 *
 * `typeLabel` is attached here so the template never recomputes it.
 */
export function deviceRow(d: KcDevice, opts: {
	busy: boolean
	preferredId: string | null
	defaultId: string | null
}): DeviceRowModel {
	const status = deviceStatus(d)
	const paired = d.pairState === 'Paired' || d.trusted
	return {
		id: d.id,
		name: d.name,
		type: d.type,
		typeLabel: deviceTypeLabel(d.type),
		transport: d.transport,
		status,
		statusLabel: statusLabel(d),
		tone: statusTone(d),
		detail: statusDetail(d, !paired),
		transportLabel: transportLabel(d.transport),
		primary: primaryAction(d, opts.busy),
		canBeDefault: d.linkState !== 'Unpaired',
		isDefault: opts.defaultId === d.id,
		canPing: d.features.includes('ping') || status === 'connected',
		canUnpair: paired,
		canMirror: d.features.includes('ping') || status === 'connected',
		isPreferred: opts.preferredId === d.id,
	}
}

/**
 * Sort order for the list: things needing attention first, connected next,
 * everything else last. Within a band, stable by name so the list does not
 * jump around as the backend re-reads it.
 */
const BAND: Record<DeviceStatus, number> = {
	pairingByPeer: 0,
	pairingFailed: 1,
	failed: 2,
	pairing: 3,
	reconnecting: 4,
	unreachable: 5,
	discovered: 6,
	connected: 7,
}

export function compareDevices(a: KcDevice, b: KcDevice): number {
	const byBand = BAND[deviceStatus(a)] - BAND[deviceStatus(b)]
	if (byBand !== 0) return byBand
	return a.name.localeCompare(b.name)
}
