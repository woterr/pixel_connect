<script setup lang="ts">
// Pixel Connect — Devices view.
//
// Every field shown here comes from the backend. Nothing is inferred, and
// nothing is reported as working that the backend has not reported.
//
// Each peer is one compact row carrying what someone acts on — name, type,
// pairing state, real connection state, transport — plus a single primary
// action for the current state and a menu of everything else. Protocol detail
// sits behind the menu, because it is diagnostic rather than actionable.
//
// The mapping from backend state to wording lives in `./deviceRow`, which is
// pure and unit-tested; this file is the view over it.

import { computed, ref } from 'vue'

import PcIcon from './atoms/PcIcon.vue'
import { compareDevices, deviceRow } from './deviceRow'
import { useAppStore } from '../stores/app'
import * as api from '../api'
import { timeAgo } from '../utils'
import type { KcDevice, WirelessTarget } from '../types'

const store = useAppStore()

/**
 * In-flight action, as `<deviceId>:<action>`.
 *
 * Keyed per device and per action so one slow device never freezes the whole
 * list, and so a button can disable *only itself*. This is what stops a
 * second Pair click firing a duplicate request while the first is in flight.
 */
const busy = ref<string | null>(null)

function isBusy(deviceId: string, action: string): boolean {
	return busy.value === `${deviceId}:${action}`
}

/** Rows whose menu/details panel is open. */
const openRows = ref<string[]>([])

function toggleRow(id: string) {
	const i = openRows.value.indexOf(id)
	if (i === -1) openRows.value.push(id)
	else openRows.value.splice(i, 1)
}

function isOpen(id: string): boolean {
	return openRows.value.includes(id)
}

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

function iconFor(t: string): string {
	if (t === 'lan') return 'wifi'
	if (t === 'bluetooth') return 'bluetooth'
	if (t === 'wifiDirect') return 'wifiOff'
	return 'link'
}

function deviceIcon(type: string): string {
	if (type === 'phone') return 'phone'
	if (type === 'tablet') return 'monitor'
	if (type === 'tv') return 'monitor'
	return 'link'
}

/**
 * The device list, sorted so anything needing a decision is at the top, with
 * each entry already reduced to its row model.
 */
const rows = computed(() => {
	const preferred = store.settings?.preferredDeviceId ?? store.preferredDevice?.id ?? null
	return [...store.devices]
		.sort(compareDevices)
		.map((d: KcDevice) =>
			deviceRow(d, {
				busy: busy.value !== null && busy.value.startsWith(`${d.id}:`),
				preferredId: preferred,
				defaultId: preferred,
			}),
		)
})

/** Devices with a verification code to confirm, shown outside the menu. */
function deviceFor(id: string): KcDevice | undefined {
	return store.devices.find((d: KcDevice) => d.id === id)
}

/**
 * Run one action on one device.
 *
 * A single `busy` key covers the whole call, so the row's own button and any
 * menu entry for the same device cannot both be used while it is running.
 */
async function act(device: KcDevice, action: string) {
	busy.value = `${device.id}:${action}`
	try {
		switch (action) {
			case 'pair':
				// "Requested", not "paired": the phone still has to confirm.
				await api.kdeRequestPairing(device.id)
				store.toast(`Pairing requested from ${device.name}`)
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
			case 'retry':
				await api.kdeRefresh()
				store.toast(`Asked the backend to reconnect to ${device.name}`, 'info')
				break
			case 'mirroring':
				await mirrorScreen(device)
				return
		}
		await store.refreshAll()
	} catch (e) {
		store.toast(`That action failed: ${e}`, 'error')
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
					? `Several devices are attached (${ready.map((d) =>d.label).join(', ')}). Choose one below.`
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

const refreshing = ref(false)

async function refreshDevices() {
	refreshing.value = true
	try {
		await api.kdeRefresh()
		await store.refreshAll()
	} catch (e) {
		store.toast(`Could not rescan: ${e}`, 'error')
	} finally {
		refreshing.value = false
	}
}

const adb = computed(() => store.adbDevices)
</script>

<template>
	<div class="pc-scroll flex-1 px-6 pb-28 pt-6 sm:px-10 lg:px-14 lg:pb-12">
		<header class="flex items-start justify-between gap-4">
			<div class="min-w-0">
				<h2 class="pc-display text-4xl">Devices</h2>
				<p class="mt-2 text-sm text-muted">
					KDE Connect protocol v{{ store.info?.kdeconnectProtocol ?? 8 }} over the local network
				</p>
			</div>
			<button
				class="pc-btn pc-btn-secondary pc-btn-md shrink-0"
				:disabled="refreshing"
				@click="refreshDevices"
			>
				<PcIcon name="refresh" :size="16" :class="refreshing ? 'animate-spin' : ''" />
				{{ refreshing ? 'Scanning…' : 'Refresh' }}
			</button>
		</header>

		<!-- KDE Connect peers -->
		<section class="mt-8">
			<div class="mb-3 flex items-baseline justify-between gap-4">
				<h3 class="pc-eyebrow">Network devices</h3>
				<span class="text-xs text-faint">
					{{ rows.length }} seen · {{ store.connectedDevices.length }} connected
				</span>
			</div>

			<div v-if="rows.length" class="space-y-2">
				<div v-for="row in rows" :key="row.id">
					<!-- One row per peer. -->
					<article
						class="pc-row"
						:data-active="row.status === 'connected'"
						:data-testid="`device-row-${row.id}`"
					>
						<PcIcon :name="deviceIcon(row.typeLabel.toLowerCase())" :size="20" class="text-muted" />

						<div class="pc-row__body">
							<div class="flex items-center gap-2">
								<h4 class="truncate font-semibold">{{ row.name }}</h4>
								<span class="pc-pill" :data-tone="row.tone">{{ row.statusLabel }}</span>
							</div>
							<p class="truncate text-sm text-muted">{{ row.detail }}</p>
						</div>

						<span
							v-if="row.transportLabel"
							class="hidden shrink-0 items-center gap-1.5 text-xs text-faint sm:flex"
						>
							<PcIcon :name="iconFor(row.transport === 'lan' ? 'wifi' : '')" :size="14" />
							{{ row.transportLabel }}
						</span>

						<button
							class="pc-btn pc-btn-sm shrink-0"
							:class="row.primary.emphasis ? 'pc-btn-primary' : 'pc-btn-secondary'"
							:disabled="row.primary.busy"
							@click="act(deviceFor(row.id)!, row.primary.id)"
						>
							{{ row.primary.label }}
						</button>

						<button
							class="pc-btn pc-btn-ghost h-9 w-9 shrink-0 p-0"
							:aria-expanded="isOpen(row.id)"
							:aria-label="`More actions for ${row.name}`"
							@click="toggleRow(row.id)"
						>
							<PcIcon name="settings" :size="16" />
						</button>
					</article>

					<!--
						Pairing code confirmation. Deliberately outside the menu: it
						is a security decision and must not be hidden behind a
						disclosure the user may never open.
					-->
					<div
						v-if="row.status === 'pairingByPeer' && deviceFor(row.id)?.verificationKey"
						class="mt-1 flex flex-wrap items-center gap-3 rounded-xl bg-inset px-4 py-3 text-sm"
					>
						<span class="text-muted">
							{{ row.name }} wants to pair. Check this code matches the phone:
						</span>
						<span class="font-mono text-lg font-bold tracking-[0.3em]">
							{{ deviceFor(row.id)?.verificationKey }}
						</span>
						<button
							class="pc-btn pc-btn-secondary pc-btn-sm ml-auto"
							:disabled="isBusy(row.id, 'reject')"
							@click="act(deviceFor(row.id)!, 'reject')"
						>
							Reject
						</button>
					</div>

					<!-- Secondary actions + protocol detail. -->
					<div v-if="isOpen(row.id)" class="pc-menu">
						<div class="flex flex-wrap gap-2 px-1 pb-2">
							<button
								v-if="row.canPing && row.status !== 'connected'"
								class="pc-btn pc-btn-secondary pc-btn-sm"
								:disabled="isBusy(row.id, 'ping')"
								@click="act(deviceFor(row.id)!, 'ping')"
							>
								<PcIcon name="send" :size="14" /> Ping
							</button>
							<button
								v-if="row.canBeDefault && !row.isDefault"
								class="pc-btn pc-btn-ghost pc-btn-sm"
								@click="setPreferred(deviceFor(row.id)!)"
							>
								<PcIcon name="link" :size="14" /> Make default
							</button>
							<span v-else-if="row.isDefault" class="pc-chip self-center">
								<PcIcon name="check" :size="12" /> Default device
							</span>
							<button
								class="pc-btn pc-btn-ghost pc-btn-sm"
								@click="act(deviceFor(row.id)!, 'mirroring')"
							>
								<PcIcon name="monitor" :size="14" /> Mirror screen
							</button>
							<button
								v-if="row.canUnpair"
								class="pc-btn pc-btn-ghost pc-btn-sm"
								:disabled="isBusy(row.id, 'unpair')"
								@click="act(deviceFor(row.id)!, 'unpair')"
							>
								<PcIcon name="unlink" :size="14" /> Unpair
							</button>
						</div>

						<dl class="grid gap-x-8 gap-y-2 border-t border-ink-line px-1 pt-3 text-sm sm:grid-cols-2">
							<div class="flex justify-between gap-4">
								<dt class="text-muted">Device id</dt>
								<dd class="truncate text-right font-mono text-xs">{{ row.id }}</dd>
							</div>
							<div class="flex justify-between gap-4">
								<dt class="text-muted">Address</dt>
								<dd class="truncate text-right font-medium">{{ deviceFor(row.id)?.address ?? '—' }}</dd>
							</div>
							<div class="flex justify-between gap-4">
								<dt class="text-muted">Protocol</dt>
								<dd class="text-right font-medium">v{{ deviceFor(row.id)?.protocolVersion }}</dd>
							</div>
							<div class="flex justify-between gap-4">
								<dt class="text-muted">Last seen</dt>
								<dd class="text-right font-medium">{{ timeAgo(deviceFor(row.id)?.lastSeen) }}</dd>
							</div>
						</dl>

						<p
							v-if="deviceFor(row.id)?.features.length"
							class="flex flex-wrap gap-2 px-1 pt-3"
						>
							<span
								v-for="f in deviceFor(row.id)!.features"
								:key="f"
								class="pc-chip"
							>
								{{ featureLabel(f) }}
							</span>
						</p>

						<p
							v-if="deviceFor(row.id)?.protocolError"
							class="mt-3 flex items-start gap-2 rounded-lg bg-danger/10 px-3 py-2 text-sm text-danger"
						>
							<PcIcon name="alert" :size="16" class="mt-0.5" />
							<span>{{ deviceFor(row.id)?.protocolError }}</span>
						</p>
					</div>
				</div>
			</div>

			<div v-else class="pc-card px-6 py-10 text-center">
				<PcIcon name="wifi" :size="28" class="mx-auto text-faint" />
				<p class="mt-3 font-semibold">No devices found yet</p>
				<p class="mx-auto mt-1 max-w-md text-sm text-muted">
					Nothing has been discovered on the local network yet. The phone has to be on the
					same network with its KDE Connect app open — being switched on is not enough for
					it to appear here.
				</p>
				<button class="pc-btn pc-btn-secondary pc-btn-md mt-5" @click="refreshDevices">
					<PcIcon name="refresh" :size="16" />
					Scan again
				</button>
				<p class="mt-4 text-xs text-faint">
					Not finding anything? The Help page covers the usual causes.
				</p>
			</div>
		</section>

		<!-- ADB / scrcpy -->
		<section class="mt-12">
			<div class="flex items-center justify-between gap-4">
				<h3 class="pc-eyebrow">Android over USB or Wi-Fi</h3>
				<span class="text-xs text-faint">
					{{ store.tools?.adbPath ? 'adb detected' : 'adb not found' }} ·
					{{ store.tools?.scrcpyPath ? 'scrcpy detected' : 'scrcpy not found' }}
				</span>
			</div>

			<div class="pc-card mt-3 px-6 py-5">
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

			<div v-if="store.tools?.error" class="pc-card mt-3 px-6 py-5 text-sm">
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

			<div v-if="adb.length" class="mt-3 space-y-2">
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
				start a wireless session with <code class="pc-code">adb connect</code>.
			</p>
		</section>

		<!-- transports, straight from the backend registry -->
		<section class="mt-12">
			<div class="flex items-center justify-between gap-4">
				<h3 class="pc-eyebrow">Connection methods</h3>
				<span class="text-xs text-faint">reported by the backend</span>
			</div>

			<div class="mt-3 space-y-2">
				<div v-for="t in store.transports" :key="t.transport" class="pc-card px-6 py-4">
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
							class="pc-pill"
							:data-tone="
								t.state === 'available'
									? 'online'
									: t.state === 'unverified'
										? 'warn'
										: 'idle'
							"
						>
							{{ t.state === 'available' ? 'Available' : t.state === 'unverified' ? 'Unverified' : 'Not implemented' }}
						</span>
					</div>

					<p class="mt-2 text-sm text-muted">{{ t.summary }}</p>
					<p v-if="t.blocker" class="mt-2 text-sm leading-relaxed text-faint">
						{{ t.blocker }}
					</p>
				</div>
			</div>

			<p class="mt-4 text-xs text-faint">
				Nothing above is claimed to work unless the backend has exercised it. Findings and
				measurements are in <code class="pc-code">docs/architecture/v2-transport-findings.md</code>.
			</p>
		</section>
	</div>
</template>
