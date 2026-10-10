<script setup lang="ts">
// Pixel Connect — Devices view.
//
// The unified device overview required by the V1 spec. Every field shown here
// comes from the backend:
//
//   identity, available protocols, pairing state, connection state, features,
//   transport, errors and recovery actions.
//
// Anything the backend has not implemented is shown as "unavailable" rather
// than as a working control.

import { computed, ref } from 'vue'

import PcIcon from './atoms/PcIcon.vue'
import { useAppStore } from '../stores/app'
import * as api from '../api'
import { deviceTypeLabel, timeAgo } from '../utils'
import type { KcDevice, WirelessTarget } from '../types'

const store = useAppStore()
const busy = ref<string | null>(null)

const FEATURE_LABELS: Record<string, string> = {
	clipboard: 'Clipboard sync',
	receiveFiles: 'Receive files',
	shareFiles: 'Send files',
	ping: 'Ping',
	battery: 'Battery',
	connectivity: 'Connectivity',
	notifications: 'Notifications',
}

function featureLabel(f: string): string {
	return FEATURE_LABELS[f] ?? f
}

/**
 * The connection state, in the order someone acts on it.
 *
 * A connected device is never labelled "Not paired", whatever the trust store
 * says: a link that is carrying traffic is connected, and claiming otherwise
 * sends the user looking for a pairing button that is not the problem. Trust
 * is reported separately, because the two genuinely differ — the phone may
 * tolerate an unpaired link while this app has not been told to trust it.
 */
function stateLabel(d: KcDevice): string {
	if (d.linkState === 'Connected') return `Connected over ${d.transport}`
	if (d.linkState === 'Connecting') return 'Connecting…'
	if (d.linkState === 'PairingRequested') return 'Pairing requested'
	if (d.linkState === 'PairingRequestedByPeer') return 'Pairing requested by the phone'
	if (d.trusted) return 'Paired, not connected'
	return 'Not paired'
}

/**
 * True when the link is up but the device is not in the trust store.
 *
 * Worth calling out separately, because it looks like a contradiction on
 * screen: transfers work, yet the app does not consider the phone paired.
 * That happens when the phone is paired with *another* KDE Connect
 * implementation - each keeps its own trust store - so the phone accepts the
 * connection while this app has nothing to pin a certificate against.
 */
function isUntrustedButConnected(d: KcDevice): boolean {
	return d.linkState === 'Connected' && !d.trusted
}

/**
 * The one line under the device name.
 *
 * Connection state is what someone acts on, so that leads. Battery is appended
 * only when the phone actually reports it, because the field is absent until a
 * `kdeconnect.battery` packet arrives and showing a bare "0%" reads as "the
 * phone is dead" when it really means "no data yet".
 */
function headline(d: KcDevice): string {
	const parts = [stateLabel(d)]
	if (d.batteryLevel !== null && d.batteryLevel !== undefined) {
		parts.push(`${d.batteryLevel}%${d.batteryIsCharging ? ' ⚡' : ''}`)
	}
	// Say so, rather than leaving "Connected" to imply everything is fine.
	// Transfers work because the phone tolerates the link, not because this app
	// has pinned a certificate for it.
	if (isUntrustedButConnected(d)) parts.push('not paired here')
	if (!d.trusted) parts.unshift(deviceTypeLabel(d.type))
	return parts.join(' · ')
}

function stateTone(d: KcDevice): string {
	if (d.linkState === 'Connected') return 'bg-online'
	if (d.linkState === 'Connecting') return 'bg-warn animate-pulse'
	if (d.trusted) return 'bg-faint'
	return 'bg-danger'
}

async function act(device: KcDevice, action: 'pair' | 'accept' | 'reject' | 'unpair' | 'ping' | 'mirroring') {
	busy.value = `${device.id}:${action}`
	try {
		switch (action) {
			case 'pair':
				await api.kdeRequestPairing(device.id)
				store.toast(`Pairing request sent to ${device.name}`)
				break
			case 'accept':
				await api.kdeAcceptPairing(device.id)
				store.toast(`${device.name} is now paired`, 'success')
				break
			case 'reject':
				await api.kdeRejectPairing(device.id)
				store.toast('Pairing rejected')
				break
			case 'unpair':
				await api.kdeUnpair(device.id)
				store.toast(`${device.name} unpaired`)
				break
			case 'ping':
				await api.kdePing(device.id, 'Pixel Connect')
				store.toast(`Ping sent to ${device.name}`, 'success')
				break
			case 'mirroring': {
				await mirrorScreen(device)
				return
			}
		}
		store.refreshAll()
	} catch (e) {
		store.toast(`Action failed: ${e}`, 'error')
	} finally {
		busy.value = null
	}
}

async function setPreferred(device: KcDevice) {
	if (!store.settings) return
	const next = { ...store.settings, preferredDeviceId: device.id }
	try {
		store.settings = await api.updateSettings(next)
		store.toast(`${device.name} is now the default device`, 'success')
	} catch (e) {
		store.toast(`Could not save the preference: ${e}`, 'error')
	}
}

// ---- ADB / scrcpy --------------------------------------------------------

/**
 * Mirror a paired phone's screen with scrcpy.
 *
 * scrcpy talks to Android over ADB, not over KDE Connect, so this has to find
 * the ADB device that corresponds to the phone. Matching is done on the device
 * name/model with an underscore/dash-insensitive comparison, because ADB
 * reports "Pixel_8" where KDE Connect reports "Pixel 8".
 *
 * When exactly one authorised device is attached there is no ambiguity and it
 * is used directly. Otherwise the user is told what is actually attached
 * rather than being sent to another screen to find out.
 */
async function mirrorScreen(device: KcDevice) {
	const wanted = device.name.toLowerCase().replace(/[_\s-]/g, '')
	const ready = store.adbDevices.filter((d) => d.state === 'device' && d.scrcpyReady)

	const exact = ready.find((d) =>
		[d.model, d.product, d.device]
			.filter((x): x is string => !!x)
			.some((x) => x.toLowerCase().replace(/[_\s-]/g, '') === wanted),
	)

	const target = exact ?? (ready.length === 1 ? ready[0] : null)

	if (!target) {
		const attached = store.adbDevices.length
		store.toast(
			attached === 0
				? 'Connect the phone by USB and enable USB debugging to mirror its screen.'
				: ready.length > 1
					? `Several devices are attached (${ready.map((d) => d.label).join(', ')}). Choose one below.`
					: 'The attached device is not ready for scrcpy. Check the USB debugging authorisation prompt.',
			'error',
		)
		return
	}

	await launchScrcpy(target.serial)
}

const wirelessTargets = ref<WirelessTarget[]>([])
const wirelessBusy = ref<string | null>(null)

/** Ask the phone, over mDNS, for any wireless-debugging endpoints. */
async function scanWireless() {
	wirelessBusy.value = 'scan'
	try {
		const found = await api.adbListWireless()
		wirelessTargets.value = found
		if (!found.length) {
			store.toast(
				'Nothing advertised. Turn on Wireless debugging on the phone and keep it in the foreground.',
				'info',
			)
		}
	} catch (e) {
		store.toast(`Wireless scan failed: ${e}`, 'error')
	} finally {
		wirelessBusy.value = null
	}
}

/** Connect to a discovered endpoint, then refresh the device list. */
async function connectWireless(target: WirelessTarget) {
	wirelessBusy.value = target.address
	try {
		const msg = await api.adbConnectWireless(target.address)
		store.toast(msg, 'success')
		await store.refreshAll()
	} catch (e) {
		store.toast(`Could not connect: ${e}`, 'error')
	} finally {
		wirelessBusy.value = null
	}
}

async function launchScrcpy(serial: string) {
	busy.value = `adb:${serial}`
	try {
		const launch = await api.scrcpyLaunch(serial, [])
		store.toast(`Started ${launch.command}`, 'success')
		store.scrcpySessions = await api.scrcpyRunning()
	} catch (e) {
		store.toast(`Could not start scrcpy: ${e}`, 'error')
	} finally {
		busy.value = null
	}
}

async function stopScrcpy(serial: string) {
	await api.scrcpyStop(serial)
	store.scrcpySessions = await api.scrcpyRunning()
}

async function refreshDevices() {
	await api.kdeRefresh()
	await store.refreshAll()
	store.toast('Re-scanned the network', 'success')
}

function iconFor(t: string): string {
	if (t === 'lan') return 'wifi'
	if (t === 'bluetooth') return 'bluetooth'
	if (t === 'wifiDirect') return 'wifiOff'
	return 'link'
}

const discovered = computed(() => store.devices)
const adb = computed(() => store.adbDevices)
</script>

<template>
	<div class="pc-scroll flex-1 px-6 pb-28 pt-6 sm:px-10 lg:px-14 lg:pb-12">
		<header class="flex items-center justify-between">
			<div>
				<h2 class="pc-display text-4xl">Devices</h2>
				<p class="mt-2 text-sm text-muted">
					KDE Connect protocol v{{ store.info?.kdeconnectProtocol ?? 8 }} over the local network
				</p>
			</div>
			<button class="pc-btn pc-btn-secondary pc-btn-md" @click="refreshDevices">
				<PcIcon name="refresh" :size="16" />
				Refresh
			</button>
		</header>

		<!-- KDE Connect devices -->
		<section class="mt-10">
			<h3 class="pc-eyebrow mb-4">Network devices</h3>

			<div v-if="discovered.length" class="space-y-4">
				<article
					v-for="device in discovered"
					:key="device.id"
					class="pc-card p-6"
				>
					<div class="flex flex-wrap items-start justify-between gap-4">
						<div class="flex items-center gap-3">
							<span class="pc-status-dot" :class="stateTone(device)" />
							<div>
								<h4 class="text-lg font-semibold">{{ device.name }}</h4>
								<p class="text-sm text-muted">{{ headline(device) }}</p>
							</div>
						</div>
						<div class="flex flex-wrap gap-2">
							<button
								v-if="!device.trusted && device.linkState !== 'PairingRequestedByPeer'"
								class="pc-btn pc-btn-primary pc-btn-sm"
								:disabled="busy === `${device.id}:pair`"
								@click="act(device, 'pair')"
							>
								Pair
							</button>
							<template v-if="device.linkState === 'PairingRequestedByPeer'">
								<button
									class="pc-btn pc-btn-primary pc-btn-sm"
									:disabled="busy === `${device.id}:accept`"
									@click="act(device, 'accept')"
								>
									Accept
									<span v-if="device.verificationKey" class="ml-1 tabular-nums">
										{{ device.verificationKey }}
									</span>
								</button>
								<button
									class="pc-btn pc-btn-secondary pc-btn-sm"
									@click="act(device, 'reject')"
								>
									Reject
								</button>
							</template>
							<button
								v-if="device.trusted"
								class="pc-btn pc-btn-secondary pc-btn-sm"
								@click="act(device, 'ping')"
							>
								Ping
							</button>
							<button
								v-if="device.trusted"
								class="pc-btn pc-btn-ghost pc-btn-sm"
								@click="act(device, 'unpair')"
							>
								Unpair
							</button>
						</div>
					</div>

					<!-- pairing verification key -->
					<div
						v-if="device.linkState === 'PairingRequestedByPeer' && device.verificationKey"
						class="pc-inset mt-4 px-4 py-3 text-sm"
					>
						Confirm that the phone shows the same code:
						<span class="ml-2 font-mono text-base font-bold tracking-widest">
							{{ device.verificationKey }}
						</span>
					</div>

					<!--
						Protocol-level detail (transport, address, device id, the
						capability chips) is diagnostic, not something a user needs
						to act on. It is hidden behind a toggle so the card shows
						only what someone would use to decide what to do next.
					-->
					<details class="mt-4">
						<summary class="cursor-pointer text-xs text-faint hover:text-muted">
							Connection details
						</summary>
						<dl class="mt-3 grid gap-x-8 gap-y-3 text-sm sm:grid-cols-2">
							<div class="flex justify-between gap-4">
								<dt class="text-muted">Transport</dt>
								<dd class="text-right font-medium">{{ device.transport }}</dd>
							</div>
							<div class="flex justify-between gap-4">
								<dt class="text-muted">Address</dt>
								<dd class="truncate text-right font-medium">{{ device.address ?? '—' }}</dd>
							</div>
							<div class="flex justify-between gap-4">
								<dt class="text-muted">Protocol</dt>
								<dd class="text-right font-medium">v{{ device.protocolVersion }}</dd>
							</div>
							<div class="flex justify-between gap-4">
								<dt class="text-muted">Last seen</dt>
								<dd class="text-right font-medium">{{ timeAgo(device.lastSeen) }}</dd>
							</div>
							<div class="flex justify-between gap-4">
								<dt class="text-muted">Device id</dt>
								<dd class="truncate text-right font-mono text-xs">{{ device.id }}</dd>
							</div>
						</dl>
						<div v-if="device.features.length" class="mt-3 flex flex-wrap gap-2">
							<span v-for="f in device.features" :key="f" class="pc-chip">
								{{ featureLabel(f) }}
							</span>
						</div>
					</details>

					<!-- errors / recovery -->
					<div
						v-if="device.protocolError"
						class="mt-5 flex items-start gap-3 rounded-xl bg-danger/10 px-4 py-3 text-sm text-danger"
					>
						<PcIcon name="alert" :size="16" class="mt-0.5" />
						<div>
							<p class="font-semibold">Connection problem</p>
							<p class="mt-1">{{ device.protocolError }}</p>
							<button
								class="pc-btn pc-btn-secondary pc-btn-sm mt-3"
								@click="refreshDevices"
							>
								Try again
							</button>
						</div>
					</div>

					<div class="mt-5 flex gap-2">
						<button
							class="pc-btn pc-btn-ghost pc-btn-sm"
							@click="setPreferred(device)"
						>
							<PcIcon :name="store.settings?.preferredDeviceId === device.id ? 'check' : 'link'" :size="14" />
							{{ store.settings?.preferredDeviceId === device.id
								? 'Default device'
								: 'Make default' }}
						</button>
						<button
							class="pc-btn pc-btn-ghost pc-btn-sm"
							:disabled="!device.features.includes('ping') && device.linkState !== 'Connected'"
							@click="act(device, 'mirroring')"
						>
							<PcIcon name="monitor" :size="14" />
							Mirror screen
						</button>
					</div>
				</article>
			</div>

			<div v-else class="pc-card px-6 py-10 text-center">
				<PcIcon name="wifi" :size="28" class="mx-auto text-faint" />
				<p class="mt-3 font-semibold">No devices found yet</p>
				<p class="mt-1 text-sm text-muted">
					Pixel Connect is listening on the local network. Make sure the phone is on the
					same Wi-Fi and has KDE Connect open, then refresh.
				</p>
				<button class="pc-btn pc-btn-secondary pc-btn-md mt-5" @click="refreshDevices">
					<PcIcon name="refresh" :size="16" />
					Scan again
				</button>
			</div>
		</section>

		<!-- ADB / scrcpy -->
		<section class="mt-14">
			<div class="flex items-center justify-between">
				<h3 class="pc-eyebrow">Android over USB or Wi-Fi</h3>
				<span class="text-xs text-faint">
					{{ store.tools?.adbPath ? 'adb detected' : 'adb not found' }} ·
					{{ store.tools?.scrcpyPath ? 'scrcpy detected' : 'scrcpy not found' }}
				</span>
			</div>

			<div class="pc-card mt-4 px-6 py-5">
				<div class="flex flex-wrap items-center justify-between gap-3">
					<div>
						<p class="font-semibold">Wireless</p>
						<p class="mt-0.5 text-sm text-muted">
							Mirror the phone without a cable. On the phone open
							<span class="font-medium">Settings → Developer options → Wireless debugging</span>
							and leave it showing.
						</p>
					</div>
					<button
						class="pc-btn pc-btn-secondary pc-btn-sm"
						:disabled="wirelessBusy === 'scan'"
						@click="scanWireless"
					>
						{{ wirelessBusy === 'scan' ? 'Searching…' : 'Find my phone' }}
					</button>
				</div>

				<div v-if="wirelessTargets.length" class="mt-4 space-y-2">
					<div
						v-for="t in wirelessTargets"
						:key="t.address + t.kind"
						class="flex flex-wrap items-center justify-between gap-3 rounded-xl bg-inset px-4 py-3"
					>
						<div class="min-w-0">
							<p class="text-sm font-medium">
								{{ t.name ?? 'Android device' }}
								<span class="ml-2 text-xs text-faint">{{ t.address }}</span>
							</p>
							<p v-if="t.kind === 'pair'" class="text-xs text-warn">
								Not paired yet — pair it once from Wireless debugging on the phone,
								then scan again.
							</p>
						</div>
						<button
							v-if="t.kind === 'connect'"
							class="pc-btn pc-btn-primary pc-btn-sm"
							:disabled="wirelessBusy === t.address"
							@click="connectWireless(t)"
						>
							Connect
						</button>
					</div>
				</div>
			</div>

<div v-if="store.tools?.error" class="pc-card mt-4 px-6 py-5 text-sm">
				<div class="flex items-start gap-3">
					<PcIcon name="alert" :size="18" class="mt-0.5 text-warn" />
					<div>
						<p class="font-semibold">Missing dependencies</p>
						<p class="mt-1 text-muted">{{ store.tools.error }}</p>
						<code class="pc-inset mt-3 block px-3 py-2 text-xs">
							sudo dnf install android-tools scrcpy
						</code>
					</div>
				</div>
			</div>

			<div v-if="adb.length" class="mt-4 space-y-3">
				<div
					v-for="device in adb"
					:key="device.serial"
					class="pc-card flex flex-wrap items-center gap-4 px-5 py-4"
				>
					<PcIcon
						:name="device.kind === 'wireless' ? 'wifi' : 'phone'"
						:size="20"
						class="text-muted"
					/>
					<div class="min-w-0 flex-1">
						<p class="truncate font-semibold">{{ device.label }}</p>
						<p class="text-sm text-muted">
							{{ device.serial }} · adb state: {{ device.state }}
						</p>
					</div>
					<button
						v-if="device.scrcpyReady"
						class="pc-btn pc-btn-primary pc-btn-sm"
						:disabled="busy === `adb:${device.serial}`"
						@click="launchScrcpy(device.serial)"
					>
						<PcIcon name="monitor" :size="14" />
						Open in scrcpy
					</button>
					<button
						v-else-if="store.scrcpySessions.includes(device.serial)"
						class="pc-btn pc-btn-secondary pc-btn-sm"
						@click="stopScrcpy(device.serial)"
					>
						Stop scrcpy
					</button>
					<p
						v-else-if="device.scrcpyBlockReason"
						class="max-w-sm text-right text-xs text-warn"
					>
						{{ device.scrcpyBlockReason }}
					</p>
				</div>
			</div>

			<p v-else-if="!store.tools?.error" class="mt-4 text-sm text-faint">
				No ADB devices detected. Connect the phone over USB with USB debugging enabled, or
				start a wireless session with <code>adb connect</code>.
			</p>
		</section>

		<!-- transports, straight from the backend registry -->
		<section class="mt-14">
			<div class="flex items-center justify-between">
				<h3 class="pc-eyebrow">Connection methods</h3>
				<span class="text-xs text-faint">reported by the backend</span>
			</div>

			<div class="mt-4 space-y-3">
				<div v-for="t in store.transports" :key="t.transport" class="pc-card px-6 py-5">
					<div class="flex flex-wrap items-center justify-between gap-3">
						<div class="flex items-center gap-2">
							<PcIcon
								:name="iconFor(t.transport)"
								:size="18"
								:class="t.state === 'available' ? 'text-online' : 'text-faint'"
							/>
							<p class="font-semibold">{{ t.name }}</p>
						</div>
						<span
							class="pc-chip"
							:class="{
								'text-online border-online/40': t.state === 'available',
								'text-warn border-warn/40': t.state === 'unverified',
							}"
						>
							{{ t.state === 'available' ? 'Available' : t.state === 'unverified' ? 'Unverified' : 'Not implemented' }}
						</span>
					</div>

					<p class="mt-2 text-sm text-muted">{{ t.summary }}</p>
					<p v-if="t.blocker" class="mt-3 text-sm leading-relaxed text-faint">
						{{ t.blocker }}
					</p>
				</div>
			</div>

			<p class="mt-4 text-xs text-faint">
				Nothing above is claimed to work unless the backend has exercised it. Findings and
				measurements are in <code>docs/architecture/v2-transport-findings.md</code>.
			</p>
		</section>
	</div>
</template>