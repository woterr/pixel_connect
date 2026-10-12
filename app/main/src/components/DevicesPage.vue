<script setup lang="ts">
// Pixel Connect — Devices view.
//
// Every field shown here comes from the backend. Nothing is inferred, and
// nothing is reported as working that the backend has not reported.
//
// Paired peers render as rich cards (name, type, battery, method chips,
// last-seen and the honest actions for their state). USB/wireless debugging
// and scrcpy live in one merged card with a numbered stepper. Transports
// render exactly as the backend registry reports them.
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

function closeOnEsc(e: KeyboardEvent) {
	if (e.key === 'Escape') {
		openRows.value = []
		;(e.currentTarget as HTMLElement | null)?.blur?.()
	}
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

/** Human reason for an unreachable card; never invents one. */
function unreachableReason(id: string): string {
	const d = deviceFor(id)
	if (!d) return 'No longer reported by the backend.'
	if (d.protocolError) return d.protocolError
	const seen = timeAgo(d.lastSeen)
	return seen === 'never' ? 'Paired, but never seen on this network.' : `Paired, but last seen ${seen}.`
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
		<div class="mx-auto w-full max-w-content">
			<header class="flex flex-wrap items-start justify-between gap-4">
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

			<!-- Paired + discovered peers as rich cards -->
			<section class="mt-8" aria-label="Network devices">
				<div class="mb-3 flex items-baseline justify-between gap-4">
					<h3 class="pc-eyebrow">Network devices</h3>
					<span class="text-xs text-faint">
						{{ rows.length }} seen · {{ store.connectedDevices.length }} connected
					</span>
				</div>

				<div v-if="rows.length" class="space-y-3">
					<article
						v-for="row in rows"
						:key="row.id"
						class="pc-device"
						:data-state="row.status"
						:data-testid="`device-row-${row.id}`"
					>
						<div class="flex items-start gap-4">
							<span class="pc-device__disc" :data-tone="row.tone">
								<PcIcon :name="deviceIcon(row.typeLabel.toLowerCase())" :size="22" />
							</span>
							<div class="min-w-0 flex-1">
								<div class="flex flex-wrap items-center gap-2">
									<h4 class="truncate text-base font-bold">{{ row.name }}</h4>
									<span class="pc-pill" :data-tone="row.tone">{{ row.statusLabel }}</span>
									<span v-if="row.isDefault" class="pc-chip">
										<PcIcon name="check" :size="12" /> Default
									</span>
								</div>
								<div class="mt-2 flex flex-wrap gap-1.5">
									<span class="pc-chip">{{ row.typeLabel }}</span>
									<span v-if="row.transportLabel" class="pc-chip">
										<PcIcon :name="iconFor(row.transport)" :size="12" />
										{{ row.transportLabel }}
									</span>
									<span v-if="deviceFor(row.id)?.batteryLevel !== null && deviceFor(row.id)?.batteryLevel !== undefined" class="pc-chip">
										<PcIcon name="battery" :size="12" />
										{{ deviceFor(row.id)?.batteryLevel }}%{{ deviceFor(row.id)?.batteryIsCharging ? ' charging' : '' }}
									</span>
									<span class="pc-chip">Last seen {{ timeAgo(deviceFor(row.id)?.lastSeen) }}</span>
								</div>
								<p v-if="row.status === 'unreachable' || row.status === 'failed' || row.status === 'pairingFailed'" class="mt-2 text-sm text-muted">
									{{ unreachableReason(row.id) }}
								</p>
								<p v-else class="mt-2 text-sm text-muted">{{ row.detail }}</p>
							</div>
						</div>

						<!--
							Pairing code confirmation. Deliberately outside the menu: it
							is a security decision and must not be hidden behind a
							disclosure the user may never open.
						-->
						<div
							v-if="row.status === 'pairingByPeer' && deviceFor(row.id)?.verificationKey"
							class="mt-4 flex flex-wrap items-center gap-3 rounded-2xl bg-inset px-4 py-3 text-sm"
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

						<div class="mt-4 flex flex-wrap items-center gap-2">
							<button
								class="pc-btn pc-btn-md shrink-0"
								:class="row.primary.emphasis ? 'pc-btn-primary' : 'pc-btn-secondary'"
								:disabled="row.primary.busy"
								@click="act(deviceFor(row.id)!, row.primary.id)"
							>
								{{ row.primary.label }}
							</button>
							<!-- Shown only when the primary action is something else (e.g.
								Accept/Pair): for unreachable/failed devices the primary
								button already is Reconnect, and rendering both produced
								two adjacent Reconnect buttons. -->
							<button
								v-if="(row.status === 'unreachable' || row.status === 'failed') && row.primary.id !== 'retry'"
								class="pc-btn pc-btn-secondary pc-btn-md shrink-0"
								:disabled="isBusy(row.id, 'retry')"
								@click="act(deviceFor(row.id)!, 'retry')"
							>
								Reconnect
							</button>
							<button
								v-if="row.canUnpair"
								class="pc-btn pc-btn-ghost pc-btn-md shrink-0"
								:disabled="isBusy(row.id, 'unpair')"
								@click="act(deviceFor(row.id)!, 'unpair')"
							>
								<PcIcon name="unlink" :size="14" /> Unpair
							</button>
							<button
								v-if="row.canBeDefault && !row.isDefault"
								class="pc-btn pc-btn-ghost pc-btn-md shrink-0"
								@click="setPreferred(deviceFor(row.id)!)"
							>
								<PcIcon name="link" :size="14" /> Make default
							</button>
							<button
								v-if="row.canPing && row.status !== 'connected'"
								class="pc-btn pc-btn-ghost pc-btn-md shrink-0"
								:disabled="isBusy(row.id, 'ping')"
								@click="act(deviceFor(row.id)!, 'ping')"
							>
								<PcIcon name="send" :size="14" /> Ping
							</button>
							<button
								class="pc-btn pc-btn-ghost pc-btn-md shrink-0"
								@click="act(deviceFor(row.id)!, 'mirroring')"
							>
								<PcIcon name="monitor" :size="14" /> Mirror screen
							</button>
							<button
								class="pc-btn pc-btn-ghost h-10 w-10 shrink-0 p-0"
								:aria-expanded="isOpen(row.id)"
								:aria-label="`More actions for ${row.name}`"
								@click="toggleRow(row.id)"
								@keydown="closeOnEsc"
							>
								<PcIcon name="menu" :size="16" />
							</button>
						</div>

						<!-- Secondary actions + protocol detail. -->
						<div v-if="isOpen(row.id)" class="pc-menu" @keydown="closeOnEsc">
							<dl class="grid gap-x-8 gap-y-2 px-1 pt-1 text-sm sm:grid-cols-2">
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
								class="mt-3 flex items-start gap-2 rounded-xl bg-inset px-3 py-2 text-sm text-danger"
							>
								<PcIcon name="alert" :size="16" class="mt-0.5 shrink-0" />
								<span>{{ deviceFor(row.id)?.protocolError }}</span>
							</p>
						</div>
					</article>
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

			<!-- Merged ADB / scrcpy card with a numbered stepper -->
			<section class="mt-12" aria-label="Screen mirroring">
				<h3 class="pc-eyebrow">Android over USB or Wi-Fi</h3>

				<div class="pc-card mt-3 px-6 py-6">
					<div class="flex flex-wrap items-center gap-2" aria-label="Tool status">
						<span class="pc-pill" :data-tone="store.tools?.adbPath ? 'online' : 'idle'">
							adb {{ store.tools?.adbVersion ?? 'not found' }}
						</span>
						<span class="pc-pill" :data-tone="store.tools?.scrcpyPath ? 'online' : 'idle'">
							scrcpy {{ store.tools?.scrcpyVersion ?? 'not found' }}
						</span>
					</div>

					<div v-if="store.tools?.error" class="mt-4 flex items-start gap-3 rounded-2xl bg-inset px-4 py-3 text-sm">
						<PcIcon name="alert" :size="18" class="mt-0.5 shrink-0 text-warn" />
						<div class="min-w-0">
							<p class="font-semibold">Missing dependencies</p>
							<p class="mt-1 text-muted">{{ store.tools.error }}</p>
							<code class="pc-inset mt-3 block px-3 py-2 text-xs">
								sudo dnf install android-tools scrcpy
							</code>
						</div>
					</div>

					<ol class="pc-steps mt-6">
						<li class="pc-steps__item">
							<span class="pc-steps__num" aria-hidden="true">1</span>
							<div class="min-w-0 flex-1">
								<p class="font-semibold">Prepare the phone</p>
								<p class="mt-1 text-sm text-muted">
									Open <span class="font-medium text-ink">Settings → Developer options → Wireless debugging</span>
									on the phone and leave it showing. For a cable, enable USB debugging instead.
								</p>
							</div>
						</li>
						<li class="pc-steps__item">
							<span class="pc-steps__num" aria-hidden="true">2</span>
							<div class="min-w-0 flex-1">
								<p class="font-semibold">Find the phone</p>
								<p class="mt-1 text-sm text-muted">
									Ask the network what the phone is advertising for wireless debugging.
								</p>
								<button
									class="pc-btn pc-btn-secondary pc-btn-sm mt-3"
									:disabled="wirelessBusy === 'scan'"
									@click="scanWireless"
								>
									{{ wirelessBusy === 'scan' ? 'Searching…' : 'Find my phone' }}
								</button>
								<div v-if="wirelessTargets.length" class="mt-3 space-y-2">
									<div
										v-for="t in wirelessTargets"
										:key="t.address + t.kind"
										class="flex flex-wrap items-center justify-between gap-3 rounded-xl bg-inset px-4 py-3"
									>
										<div class="min-w-0">
											<p class="truncate text-sm font-medium">
												{{ t.name ?? 'Android device' }}
												<span class="ml-2 text-xs text-faint">{{ t.address }}</span>
											</p>
											<p v-if="t.kind === 'pair'" class="mt-0.5 text-xs text-warn">
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
						</li>
						<li class="pc-steps__item">
							<span class="pc-steps__num" aria-hidden="true">3</span>
							<div class="min-w-0 flex-1">
								<p class="font-semibold">Mirror the screen</p>
								<p class="mt-1 text-sm text-muted">
									Every device ADB can see, with what is blocking scrcpy when one is not ready.
								</p>
								<div v-if="adb.length" class="mt-3 space-y-2">
									<div
										v-for="device in adb"
										:key="device.serial"
										class="flex flex-wrap items-center gap-3 rounded-xl bg-inset px-4 py-3"
									>
										<PcIcon
											:name="device.kind === 'wireless' ? 'wifi' : 'phone'"
											:size="20"
											class="shrink-0 text-muted"
										/>
										<div class="min-w-0 flex-1">
											<p class="truncate text-sm font-semibold">{{ device.label }}</p>
											<p class="truncate text-xs text-muted">
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
								<p v-else-if="!store.tools?.error" class="mt-3 text-sm text-faint">
									No ADB devices detected. Connect the phone over USB with USB debugging enabled, or
									start a wireless session from step 2.
								</p>
							</div>
						</li>
					</ol>
				</div>
			</section>

			<!-- transports, straight from the backend registry -->
			<section class="mt-12" aria-label="Connection methods">
				<div class="flex items-center justify-between gap-4">
					<h3 class="pc-eyebrow">Connection methods</h3>
					<span class="text-xs text-faint">reported by the backend</span>
				</div>

				<ul class="mt-3 space-y-2">
					<li v-for="t in store.transports" :key="t.transport" class="pc-transport">
						<PcIcon
							:name="iconFor(t.transport)"
							:size="18"
							:class="t.state === 'available' ? 'text-online' : 'text-faint'"
						/>
						<div class="min-w-0 flex-1">
							<div class="flex flex-wrap items-center gap-2">
								<p class="text-sm font-semibold">{{ t.name }}</p>
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
							<p class="mt-1 text-sm text-muted">{{ t.summary }}</p>
							<details v-if="t.blocker" class="pc-why mt-2">
								<summary class="pc-why__summary">Why?</summary>
								<p class="mt-2 text-sm leading-relaxed text-muted">{{ t.blocker }}</p>
							</details>
						</div>
					</li>
				</ul>

				<p class="mt-4 text-xs text-faint">
					Nothing above is claimed to work unless the backend has exercised it. Findings and
					measurements are in <code class="pc-code">docs/architecture/v2-transport-findings.md</code>.
				</p>
			</section>
		</div>
	</div>
</template>

<style scoped>
.pc-device {
	background-color: rgb(var(--pc-card));
	border-radius: var(--pc-radius-card);
	border: 1px solid rgb(var(--pc-line));
	padding: 1.25rem 1.25rem 1rem;
}
.pc-device[data-state='connected'] {
	border-color: rgb(var(--pc-online) / 0.45);
}
.pc-device[data-state='unreachable'],
.pc-device[data-state='discovered'] {
	border-style: dashed;
}
.pc-device__disc {
	display: inline-flex;
	align-items: center;
	justify-content: center;
	flex: none;
	width: 3rem;
	height: 3rem;
	border-radius: 9999px;
	background-color: rgb(var(--pc-card-alt));
	color: rgb(var(--pc-muted));
}
.pc-device__disc[data-tone='online'] {
	background-color: rgb(var(--pc-accent));
	color: rgb(var(--pc-on-accent));
}
.pc-device__disc[data-tone='warn'] {
	color: rgb(var(--pc-warn));
}
.pc-device__disc[data-tone='danger'] {
	color: rgb(var(--pc-danger));
}

/* Numbered stepper for wireless debugging. */
.pc-steps {
	list-style: none;
	margin: 0;
	padding: 0;
	display: flex;
	flex-direction: column;
	gap: 0.25rem;
}
.pc-steps__item {
	display: flex;
	gap: 0.875rem;
	padding: 0.75rem 0;
}
.pc-steps__item + .pc-steps__item {
	border-top: 1px solid rgb(var(--pc-line));
}
.pc-steps__num {
	flex: none;
	width: 1.75rem;
	height: 1.75rem;
	margin-top: 0.125rem;
	display: inline-flex;
	align-items: center;
	justify-content: center;
	border-radius: 9999px;
	background-color: rgb(var(--pc-accent));
	color: rgb(var(--pc-on-accent));
	font-size: 0.8125rem;
	font-weight: 800;
	font-variant-numeric: tabular-nums;
}

/* Compact transport rows with an animated Why? disclosure. */
.pc-transport {
	display: flex;
	align-items: flex-start;
	gap: 0.75rem;
	background-color: rgb(var(--pc-card));
	border: 1px solid rgb(var(--pc-line));
	border-radius: var(--pc-radius-inner);
	padding: 0.875rem 1rem;
}
.pc-why__summary {
	display: inline-flex;
	align-items: center;
	gap: 0.375rem;
	cursor: pointer;
	font-size: 0.8125rem;
	font-weight: 600;
	color: rgb(var(--pc-muted));
	border-radius: 0.5rem;
}
.pc-why__summary:hover {
	color: rgb(var(--pc-text));
}
.pc-why[open] .pc-why__summary {
	color: rgb(var(--pc-text));
}

article :focus-visible,
section :focus-visible,
button:focus-visible,
summary:focus-visible {
	outline: 2px solid var(--pc-primary);
	outline-offset: 2px;
}
</style>
