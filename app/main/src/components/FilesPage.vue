<script setup lang="ts">
// Pixel Connect — Files view.
//
// The primary screen: a connection hero for the preferred phone, the transfer
// headline with the signature wavy progress rule, Send Text / Send File cards,
// incoming share requests and recent transfers. Every action goes through the
// existing API surface; nothing here invents backend state.

import { computed, onBeforeUnmount, ref, watch } from 'vue'

import PcIcon from './atoms/PcIcon.vue'
import ProgressWave from './atoms/ProgressWave.vue'
import type { ViewName } from './SideMenu.vue'
import type { PendingShare, TransferProgress } from '../types'
import { useAppStore } from '../stores/app'
import { formatBytes, formatEta, formatRate, toPercent, etaFrom, fileCategory } from '../utils'
import * as api from '../api'

const emit = defineEmits<{ navigate: [ViewName] }>()
defineProps<{ view: ViewName }>()

const store = useAppStore()

const dragActive = ref(false)
const textDraft = ref('')
const sendingText = ref(false)
const selectedFile = ref<{ name: string; path: string; size: number } | null>(null)
const reconnecting = ref(false)
const troubleshooting = ref(false)
const openMenuId = ref<string | null>(null)

const textCount = computed(() => textDraft.value.length)

// ---- connection hero -------------------------------------------------------

const heroDevice = computed(() => store.preferredDevice)
const heroConnected = computed(() => heroDevice.value?.linkState === 'Connected')
const heroName = computed(() => heroDevice.value?.name ?? 'Your phone')
const heroTransport = computed(() => {
	const t = heroDevice.value?.transport
	if (t === 'lan') return 'Wi-Fi'
	if (t === 'bluetooth') return 'Bluetooth'
	if (t === 'wifiDirect') return 'Wi-Fi Direct'
	return null
})
const heroBattery = computed(() => heroDevice.value?.batteryLevel ?? null)
const heroCharging = computed(() => heroDevice.value?.batteryIsCharging === true)

async function reconnect() {
	reconnecting.value = true
	try {
		await api.kdeRefresh()
		await store.refreshAll()
	} catch (e) {
		store.toast(`Could not rescan: ${e}`, 'error')
	} finally {
		reconnecting.value = false
	}
}

async function pingHero() {
	const target = heroDevice.value
	if (!target) return
	try {
		await api.kdePing(target.id, 'Pixel Connect')
		store.toast(`Ping sent to ${target.name}`, 'success')
	} catch (e) {
		store.toast(`That action failed: ${e}`, 'error')
	}
}

/**
 * Ring the phone so it can be found. The backend exposes no dedicated
 * find-my-phone packet, so this sends a KDE Connect ping — the mechanism the
 * phone actually surfaces as a notification — with a message that says what
 * it is for, rather than pretending a ringer API exists.
 */
async function findHero() {
	const target = heroDevice.value
	if (!target) return
	try {
		await api.kdePing(target.id, 'Find my phone — Pixel Connect is looking for you')
		store.toast(`Ringing ${target.name} — listen for the notification`, 'success')
	} catch (e) {
		store.toast(`That action failed: ${e}`, 'error')
	}
}

/**
 * Mirror the preferred phone, using the same ADB matching as the Devices
 * page: the scrcpy-ready device whose model matches, or the single attached
 * device when there is no ambiguity. Reports what is attached instead of
 * guessing when there is none.
 */
async function mirrorHero() {
	const target = heroDevice.value
	const wanted = (target?.name ?? '').toLowerCase().replace(/[_\s-]/g, '')
	const ready = store.adbDevices.filter((d) => d.state === 'device' && d.scrcpyReady)
	const exact = ready.find((d) =>
		[d.model, d.product, d.device]
			.filter((x): x is string => !!x)
			.some((x) => x.toLowerCase().replace(/[_\s-]/g, '') === wanted),
	)
	const found = exact ?? (ready.length === 1 ? ready[0] : null)
	if (!found) {
		const attached = store.adbDevices.length
		store.toast(
			attached === 0
				? 'Connect the phone by USB and enable USB debugging to mirror its screen.'
				: ready.length > 1
					? `Several devices are attached (${ready.map((d) => d.label).join(', ')}). Choose one under Devices.`
					: 'The attached device is not ready for scrcpy. Check the USB debugging authorisation prompt.',
			'error',
		)
		emit('navigate', 'devices')
		return
	}
	try {
		const launch = await api.scrcpyLaunch(found.serial, [])
		store.toast(`Started ${launch.command}`, 'success')
		store.scrcpySessions = await api.scrcpyRunning()
	} catch (e) {
		store.toast(`Could not start scrcpy: ${e}`, 'error')
	}
}

// ---- headline / progress -------------------------------------------------

const headline = computed(() => {
	if (store.quickShareReceiving) return 'Receiving files…'
	if (store.quickShareSending) return 'Sending files…'
	if (store.quickShareAwaitingConsent)
		return store.quickshareMeta?.source?.name
			? `${store.quickshareMeta.source.name} wants to share`
			: 'A device wants to share'
	if (store.kdeTransferProgress !== null) return 'Transferring files…'
	return 'Ready to share'
})

const headlineIsActive = computed(
	() =>
		store.quickShareReceiving ||
		store.quickShareSending ||
		store.kdeTransferProgress !== null,
)

const progress = computed(() => store.kdeTransferProgress ?? store.quickShareProgress)

const percentLabel = computed(() =>
	headlineIsActive.value ? `${toPercent(progress.value)}%` : '',
)

/** Screen-reader narration of live progress. */
const progressAnnouncement = computed(() => {
	if (!headlineIsActive.value) return 'No transfer in progress.'
	const file = activeFile.value
	return `${headline.value} ${toPercent(progress.value)} percent${file ? `, ${file.name}` : ''}.`
})

const activeFile = computed(() => {
	if (store.quickshareMeta) {
		const files = store.quickshareMeta.files ?? []
		return {
			name: files[0] ?? store.quickshareMeta.text_description ?? 'Shared payload',
			size: store.quickshareMeta.total_bytes,
			transferred: store.quickshareMeta.ack_bytes,
			source: store.quickshareMeta.source?.name ?? 'Nearby device',
		}
	}
	const t = store.activeTransfers[0]
	if (t) {
		return {
			name: t.filename,
			size: t.total,
			transferred: t.bytes,
			source: store.deviceName(t.deviceId),
		}
	}
	return null
})

const rate = ref<number | null>(null)
const startedAt = ref<number | null>(null)

// Approximate transfer rate from consecutive progress samples.
const lastSample = ref<{ bytes: number; at: number } | null>(null)

function trackProgress() {
	const t = store.activeTransfers[0]
	const current = t?.bytes ?? store.quickshareMeta?.ack_bytes
	const now = Date.now()
	if (current === undefined) {
		lastSample.value = null
		startedAt.value = null
		rate.value = null
		return
	}
	if (startedAt.value === null) startedAt.value = now
	const s = lastSample.value
	if (s && now > s.at && current >= s.bytes) {
		const dt = (now - s.at) / 1000
		rate.value = (current - s.bytes) / dt
	}
	lastSample.value = { bytes: current, at: now }
}

const eta = computed(() => {
	if (!progress.value || progress.value >= 1) return null
	if (rate.value && rate.value > 0) {
		const remaining = (activeFile.value?.size ?? 0) - (activeFile.value?.transferred ?? 0)
		return Math.max(0, remaining / rate.value)
	}
	return etaFrom(progress.value, startedAt.value ? Date.now() - startedAt.value : 0)
})

/** Cancel the in-flight KDE transfer shown in the active-file card. */
async function cancelActiveTransfer() {
	const t = store.activeTransfers[0]
	if (!t) return
	await store.cancelTransfer(t)
}

// ---- sending -------------------------------------------------------------

async function sendText() {
	const target = store.preferredDevice
	if (!target) {
		store.toast('No paired Pixel is available. Pair one under Devices first.', 'error')
		return
	}
	const text = textDraft.value.trim()
	if (!text) return
	sendingText.value = true
	try {
		await api.kdeShareText(target.id, text)
		textDraft.value = ''
		store.toast(`Text shared with ${target.name}`, 'success')
	} catch (e) {
		store.toast(`Could not share the text: ${e}`, 'error')
	} finally {
		sendingText.value = false
	}
}

async function sendSelected() {
	if (!selectedFile.value) return
	const target = store.preferredDevice
	if (!target) {
		store.toast('No paired Pixel is available. Pair one under Devices first.', 'error')
		return
	}
	try {
		const n = await api.kdeShareFiles(target.id, [selectedFile.value.path])
		store.toast(
			n > 0 ? `Sent ${selectedFile.value.name} to ${target.name}` : 'Nothing was sent',
			n > 0 ? 'success' : 'error',
		)
		selectedFile.value = null
	} catch (e) {
		store.toast(`Could not send the file: ${e}`, 'error')
	}
}

/**
 * Choose a file to send.
 *
 * Uses the Tauri dialog plugin rather than a hidden `<input type="file">`.
 * A plain file input is handled entirely by the web view, and this build does
 * not expose the `path` property on the resulting `File`, so the selection
 * arrived with no filesystem path and nothing could be sent - surfacing as
 * "This build of the web view did not expose a file path". The dialog plugin
 * goes through the native GTK picker and returns a real path, which is what
 * the backend needs.
 */
async function browseForFile() {
	try {
		const { open } = await import('@tauri-apps/plugin-dialog')
		const picked = await open({ multiple: false, directory: false })
		if (typeof picked !== 'string') return
		selectedFile.value = {
			name: picked.split('/').pop() || picked,
			path: picked,
			size: 0,
		}
	} catch (e) {
		store.toast(`Could not open the file chooser: ${e}`, 'error')
	}
}

function onDrop(e: DragEvent) {
	dragActive.value = false
	const f = e.dataTransfer?.files?.[0]
	if (!f) return
	// Drag and drop is genuinely unsupported here: the web view will not resolve
	// a filesystem path for a dropped file, and there is no way to recover one
	// in the browser. Say so and point at the two routes that do work.
	store.toast(
		`"${f.name}" cannot be sent by drag and drop. Use "Browse", or right-click the file in Files and choose "Send to Pixel".`,
		'info',
	)
}

function removeSelected() {
	selectedFile.value = null
}

function fileTypeLabel(name: string): string {
	switch (fileCategory(name)) {
		case 'image': return 'Image'
		case 'doc': return 'Document'
		case 'archive': return 'Archive'
		case 'media': return 'Media'
		case 'code': return 'Code'
		default: return 'File'
	}
}

function transferIcon(name: string): string {
	return fileCategory(name) === 'image' ? 'image' : 'file'
}

async function respondToRequest(accept: boolean) {
	const id = store.quickshareMeta?.id
	if (!id) return
	if (accept) await api.quickshareAcceptTransfer(id)
	else await api.quickshareRejectTransfer(id)
}

/**
 * Put shared text (or a shared link) on the desktop clipboard.
 *
 * Sharing a snippet from the phone is a request to have it ready to paste, so
 * that is the whole point of the row. The previous "Review" button dismissed
 * the offer and navigated to the Devices page, which did nothing with it.
 */
async function copySharedText(share: PendingShare) {
	const text = share.text ?? share.url
	if (!text) return
	try {
		await api.clipboardWriteText(text)
		store.toast('Copied to the clipboard', 'success')
		await api.kdeDismissShare(share.id)
		store.shares = store.shares.filter((x) => x.id !== share.id)
	} catch (e) {
		store.toast(`Could not copy: ${e}`, 'error')
	}
}

/**
 * Accept an incoming share.
 *
 * What this can and cannot do is worth being precise about. KDE Connect has no
 * consent step for a *received* file: the protocol has the receiver open the
 * payload connection and write the bytes as soon as the announcement arrives,
 * and Android's `LanLink.kt` does the same when it receives. By the time an
 * offer is in this list the file is already in the incoming folder.
 *
 * So this records the decision and clears the request. It cannot refuse the
 * transfer, and the button must not pretend otherwise. Rejecting below only
 * hides the row.
 */
async function acceptShare(share: PendingShare) {
	try {
		await api.kdeAcceptShare(share.id)
		store.shares = store.shares.filter((x) => x.id !== share.id)
		const what = share.filename ?? 'the shared item'
		store.toast(`Saved ${what} to your incoming folder`, 'success')
	} catch (e) {
		store.toast(`Could not accept that share: ${e}`, 'error')
	}
}

async function rejectShare(share: PendingShare) {
	await api.kdeDismissShare(share.id)
	store.shares = store.shares.filter((x) => x.id !== share.id)
}

async function openPendingShare(id: string) {
	await api.kdeDismissShare(id)
	emit('navigate', 'devices')
}

// ---- recent transfers ------------------------------------------------------
//
// The backend reports no timestamp and no on-disk path for a transfer, so
// rows show what is real (name, size, device, live state) and do not invent a
// time. Opening or revealing a file has no backend command, so those menu
// entries stay disabled with the reason; copying the file name works today.

const recent = computed(() =>
	store.transfers.slice().reverse().slice(0, 5),
)

async function retryTransfer(t: TransferProgress) {
	try {
		await api.kdeRefresh()
		await store.refreshAll()
		const d = store.deviceName(t.deviceId)
		store.toast(`Asked the backend to reconnect to ${d} — send ${t.filename} again once it is back`, 'info')
	} catch (e) {
		store.toast(`Could not rescan: ${e}`, 'error')
	}
}

function dismissTransfer(t: TransferProgress) {
	store.transfers = store.transfers.filter(
		(x) => !(x.deviceId === t.deviceId && x.filename === t.filename),
	)
}

async function copyTransferName(t: TransferProgress) {
	try {
		await api.clipboardWriteText(t.filename)
		store.toast('File name copied to the clipboard', 'success')
	} catch (e) {
		store.toast(`Could not copy: ${e}`, 'error')
	}
	openMenuId.value = null
}

function toggleMenu(id: string) {
	openMenuId.value = openMenuId.value === id ? null : id
}

function closeMenuOnEsc(e: KeyboardEvent) {
	if (e.key === 'Escape') {
		openMenuId.value = null
		;(e.currentTarget as HTMLElement | null)?.blur?.()
	}
}

// ---- connected pill ------------------------------------------------------

const connectedDevice = computed(() => store.connectedDevices[0] ?? null)

// Approximate throughput from consecutive progress samples. Sampling only
// runs while a transfer is active, so an idle app does no work.
let sampler: ReturnType<typeof setInterval> | null = null

watch(
	() => activeFile.value?.transferred,
	() => trackProgress(),
)

watch(
	headlineIsActive,
	(active) => {
		if (active && !sampler) {
			sampler = setInterval(trackProgress, 500)
		} else if (!active && sampler) {
			clearInterval(sampler)
			sampler = null
			startedAt.value = null
			lastSample.value = null
			rate.value = null
		}
	},
	{ immediate: true },
)

onBeforeUnmount(() => {
	if (sampler) clearInterval(sampler)
})
</script>

<template>
	<div class="pc-scroll relative flex-1 px-6 pb-28 pt-6 sm:px-10 lg:px-14 lg:pb-12">
		<div class="mx-auto w-full max-w-content">
			<!-- connection hero -->
			<section class="pc-hero" :data-state="heroConnected ? 'online' : 'offline'" aria-label="Connection">
				<template v-if="heroConnected && heroDevice">
					<div class="flex flex-wrap items-center gap-5">
						<div class="pc-battery" role="img" :aria-label="heroBattery !== null ? `Battery ${heroBattery} percent${heroCharging ? ', charging' : ''}` : 'Battery level unknown'">
							<svg viewBox="0 0 72 72" class="h-[72px] w-[72px]" aria-hidden="true">
								<circle cx="36" cy="36" r="30" fill="none" stroke="rgb(var(--pc-line))" stroke-width="6" />
								<circle
									v-if="heroBattery !== null"
									cx="36"
									cy="36"
									r="30"
									fill="none"
									stroke="rgb(var(--pc-online))"
									stroke-width="6"
									stroke-linecap="round"
									:stroke-dasharray="`${(Math.max(0, Math.min(100, heroBattery)) / 100) * 188.5} 188.5`"
									transform="rotate(-90 36 36)"
								/>
								<text x="36" y="36" text-anchor="middle" dominant-baseline="central" class="pc-battery__text">
									{{ heroBattery !== null ? `${heroBattery}%` : '–' }}
								</text>
							</svg>
						</div>
						<div class="min-w-0 flex-1">
							<p class="pc-eyebrow">Connected</p>
							<h2 class="mt-1 truncate text-2xl font-bold tracking-tight sm:text-3xl">{{ heroDevice.name }}</h2>
							<div class="mt-2 flex flex-wrap items-center gap-2">
								<span v-if="heroTransport" class="pc-pill" data-tone="online">
									<PcIcon name="wifi" :size="14" />
									{{ heroTransport }}
								</span>
								<span v-if="heroCharging" class="pc-pill" data-tone="warn">
									<PcIcon name="bolt" :size="14" />
									Charging
								</span>
								<span v-if="heroDevice.address" class="text-xs text-faint">{{ heroDevice.address }}</span>
							</div>
						</div>
						<div class="flex flex-wrap gap-2" role="group" aria-label="Quick actions">
							<button type="button" class="pc-btn pc-btn-secondary pc-btn-md" @click="pingHero">
								<PcIcon name="send" :size="16" />
								Ping
							</button>
							<button type="button" class="pc-btn pc-btn-secondary pc-btn-md" @click="findHero">
								<PcIcon name="search" :size="16" />
								Find
							</button>
							<button type="button" class="pc-btn pc-btn-primary pc-btn-md" @click="mirrorHero">
								<PcIcon name="monitor" :size="16" />
								Mirror
							</button>
						</div>
					</div>
				</template>
				<template v-else>
					<div class="flex flex-col items-center px-4 py-8 text-center sm:py-10">
						<svg viewBox="0 0 96 96" class="h-24 w-24" role="img" aria-label="Phone unreachable illustration">
							<rect x="30" y="10" width="36" height="68" rx="9" fill="none" stroke="rgb(var(--pc-faint))" stroke-width="3" />
							<line x1="42" y1="20" x2="54" y2="20" stroke="rgb(var(--pc-faint))" stroke-width="3" stroke-linecap="round" />
							<line x1="24" y1="24" x2="72" y2="72" stroke="rgb(var(--pc-danger))" stroke-width="4" stroke-linecap="round" />
							<circle cx="48" cy="66" r="3" fill="rgb(var(--pc-faint))" />
						</svg>
						<h2 class="pc-headline mt-5">{{ `${heroName} isn't reachable` }}</h2>
						<p class="mt-2 max-w-md text-sm text-muted">
							<template v-if="heroDevice">
								{{ heroName }} is paired but cannot be reached right now. It needs to be on
								the same network — the KDE Connect service on the phone runs in the
								background, so the app does not need to be open.
							</template>
							<template v-else>
								No phone has been paired yet. Pair one under Devices to start sharing.
							</template>
						</p>
						<div class="mt-6 flex flex-wrap justify-center gap-2">
							<button
								type="button"
								class="pc-btn pc-btn-primary pc-btn-md"
								:disabled="reconnecting"
								@click="reconnect"
							>
								<PcIcon name="refresh" :size="16" :class="reconnecting ? 'animate-spin' : ''" />
								{{ reconnecting ? 'Reconnecting…' : 'Reconnect' }}
							</button>
							<button
								type="button"
								class="pc-btn pc-btn-secondary pc-btn-md"
								:aria-expanded="troubleshooting"
								@click="troubleshooting = !troubleshooting"
							>
								Troubleshoot
							</button>
						</div>
						<div v-if="troubleshooting" class="mt-6 w-full max-w-lg rounded-2xl bg-inset p-5 text-left">
							<p class="pc-eyebrow">Before anything else</p>
							<ol class="mt-3 space-y-2 text-sm text-muted">
								<li class="flex gap-2"><span class="font-bold text-ink">1.</span> Put the phone on the same Wi-Fi network as this computer.</li>
								<li class="flex gap-2"><span class="font-bold text-ink">2.</span> Make sure KDE Connect is enabled on the phone (its background service advertises on its own — the app does not need to be open).</li>
								<li class="flex gap-2"><span class="font-bold text-ink">3.</span> Press Reconnect above, then look under Devices for a Pair button.</li>
							</ol>
							<div class="mt-4 flex flex-wrap gap-2">
								<button type="button" class="pc-btn pc-btn-secondary pc-btn-sm" @click="emit('navigate', 'devices')">
									Open Devices
								</button>
								<button type="button" class="pc-btn pc-btn-ghost pc-btn-sm" @click="api.openHelp()">
									Open help
								</button>
							</div>
						</div>
					</div>
				</template>
			</section>

			<!-- headline + wave -->
			<section class="mt-10" aria-label="Transfer status">
				<p class="sr-only" aria-live="polite">{{ progressAnnouncement }}</p>
				<div class="flex items-baseline justify-between gap-6">
					<h3 class="pc-display text-4xl sm:text-5xl">{{ headline }}</h3>
					<span
						v-if="percentLabel"
						class="shrink-0 text-3xl font-bold tabular-nums sm:text-4xl"
					>
						{{ percentLabel }}
					</span>
				</div>
				<div
					class="pc-wavebox"
					:data-mode="!heroConnected ? 'flat' : headlineIsActive ? 'live' : progress >= 1 && progress > 0 ? 'done' : 'idle'"
				>
					<ProgressWave class="mt-6" :value="progress" />
				</div>

				<!-- incoming request prompt -->
				<div
					v-if="store.quickShareAwaitingConsent"
					class="pc-card mt-8 flex flex-wrap items-center justify-between gap-4 px-6 py-5"
				>
					<div>
						<p class="font-semibold">
							{{ store.quickshareMeta?.source?.name ?? 'A nearby device' }} is sharing
						</p>
						<p class="mt-1 text-sm text-muted">
							{{ store.quickshareMeta?.files?.length ?? 1 }} item(s),
							{{ formatBytes(store.quickshareMeta?.total_bytes ?? 0) }}
						</p>
					</div>
					<div class="flex gap-2">
						<button class="pc-btn pc-btn-primary pc-btn-md" @click="respondToRequest(true)">
							Accept
						</button>
						<button class="pc-btn pc-btn-secondary pc-btn-md" @click="respondToRequest(false)">
							Reject
						</button>
					</div>
				</div>

				<!-- active file card -->
				<div
					v-if="activeFile && headlineIsActive"
					class="pc-card mt-8 flex items-center gap-4 px-5 py-4"
				>
					<div class="pc-inset flex h-14 w-14 items-center justify-center">
						<PcIcon
							:name="fileCategory(activeFile.name) === 'image' ? 'image' : 'file'"
							:size="22"
							class="text-muted"
						/>
					</div>
					<div class="min-w-0 flex-1">
						<p class="truncate font-semibold">{{ activeFile.name }}</p>
						<p class="mt-0.5 text-sm text-muted">
							{{ formatBytes(activeFile.size) }}
							<span v-if="rate"> · {{ formatRate(rate) }}</span>
							<span v-if="eta"> · {{ formatEta(eta) }}</span>
							<span v-else> · {{ activeFile.source }}</span>
						</p>
					</div>
					<button
						v-if="store.quickShareAwaitingConsent"
						type="button"
						class="pc-btn pc-btn-ghost h-11 w-11 p-0"
						aria-label="Cancel transfer"
						@click="respondToRequest(false)"
					>
						<PcIcon name="close" :size="20" />
					</button>
					<button
						v-else-if="store.activeTransfers[0]"
						type="button"
						class="pc-btn pc-btn-ghost h-11 w-11 p-0"
						aria-label="Cancel transfer"
						@click="cancelActiveTransfer"
					>
						<PcIcon name="close" :size="20" />
					</button>
				</div>
			</section>

			<!-- send text + send file -->
			<section class="mt-14 grid gap-6 lg:grid-cols-2" aria-label="Send">
				<!-- Send Text -->
				<div class="pc-card flex flex-col p-6">
					<div class="flex items-center gap-2">
						<PcIcon name="send" :size="18" class="text-muted" />
						<h3 class="font-semibold">Send Text</h3>
						<span class="ml-auto text-xs text-faint tabular-nums" aria-live="off">{{ textCount }} characters</span>
					</div>
					<div class="pc-float mt-4 flex-1">
						<textarea
							id="pc-send-text"
							v-model="textDraft"
							class="pc-float__input min-h-[130px] flex-1"
							placeholder=" "
							aria-label="Text to share"
							@keydown.meta.enter="sendText"
							@keydown.ctrl.enter="sendText"
						/>
						<label class="pc-float__label" for="pc-send-text">Type or paste content to share…</label>
					</div>
					<p class="mt-2 text-xs text-faint">Ctrl+Enter or Cmd+Enter sends.</p>
					<div class="pc-divider mt-2 border-t" />
					<div class="mt-4 flex justify-end">
						<button
							class="pc-btn pc-btn-primary pc-btn-md"
							:disabled="!textDraft.trim() || sendingText"
							@click="sendText"
						>
							Send
							<PcIcon name="send" :size="16" />
						</button>
					</div>
				</div>

				<!-- Send File -->
				<div class="pc-card flex flex-col p-6">
					<div class="flex items-center gap-2">
						<PcIcon name="upload" :size="18" class="text-muted" />
						<h3 class="font-semibold">Send File</h3>
					</div>
					<div
						class="pc-dropzone mt-4 flex flex-1 flex-col items-center justify-center gap-3 p-8"
						:data-active="dragActive"
						@dragenter.prevent="dragActive = true"
						@dragover.prevent="dragActive = true"
						@dragleave.prevent="dragActive = false"
						@drop.prevent="onDrop"
					>
						<div class="pc-dropicon pc-inset flex h-12 w-12 items-center justify-center rounded-full">
							<PcIcon name="plus" :size="22" />
						</div>
						<p class="text-sm text-muted">
							Drag &amp; drop or
							<button type="button" class="underline" @click="browseForFile">Browse</button>
						</p>
					</div>
					<div v-if="selectedFile" class="pc-filechip mt-4">
						<PcIcon :name="transferIcon(selectedFile.name)" :size="20" class="shrink-0 text-muted" />
						<div class="min-w-0 flex-1">
							<p class="truncate text-sm font-semibold">{{ selectedFile.name }}</p>
							<p class="mt-0.5 text-xs text-muted">
								{{ fileTypeLabel(selectedFile.name) }}
								<span v-if="selectedFile.size > 0"> · {{ formatBytes(selectedFile.size) }}</span>
							</p>
						</div>
						<button
							type="button"
							class="pc-btn pc-btn-ghost h-9 w-9 shrink-0 p-0"
							aria-label="Remove selected file"
							@click="removeSelected"
						>
							<PcIcon name="close" :size="16" />
						</button>
					</div>
					<p v-else class="mt-4 rounded-xl bg-ink-card-alt px-4 py-3 text-center text-sm text-muted">
						No file selected yet
					</p>
					<button
						class="pc-btn pc-btn-secondary mt-3 w-full py-3"
						:disabled="!selectedFile"
						@click="sendSelected"
					>
						Send File
						<PcIcon name="upload" :size="16" />
					</button>
				</div>
			</section>

			<!-- pending incoming shares -->
			<section v-if="store.shares.length" class="mt-14" aria-label="Incoming shares">
				<h3 class="pc-eyebrow mb-3">Incoming shares</h3>
				<div class="space-y-3">
					<div
						v-for="share in store.shares"
						:key="share.id"
						class="pc-card flex items-center gap-4 px-5 py-4"
					>
						<div class="pc-inset flex h-12 w-12 shrink-0 items-center justify-center">
							<PcIcon name="download" :size="20" class="text-muted" />
						</div>
						<div class="min-w-0 flex-1">
							<p class="truncate font-semibold">
								{{ share.filename ?? (share.kind === 'text' ? 'Shared text' : share.kind) }}
							</p>
							<!--
								Show what was actually shared. A row reading only "text" tells
								the user nothing, and "Review" then did nothing with it.
							-->
							<p
								v-if="share.text"
								class="mt-1 line-clamp-2 whitespace-pre-wrap break-words text-sm text-muted"
							>
								{{ share.text }}
							</p>
							<p v-else-if="share.url" class="mt-1 truncate text-sm text-muted">
								{{ share.url }}
							</p>
							<p class="mt-1 text-xs text-faint">
								From {{ share.deviceName }}
								<span v-if="share.filesize"> · {{ formatBytes(share.filesize) }}</span>
							</p>
						</div>
						<div class="flex shrink-0 flex-wrap justify-end gap-2">
							<button
								v-if="share.text || share.url"
								class="pc-btn pc-btn-primary pc-btn-sm"
								@click="copySharedText(share)"
							>
								Copy to clipboard
							</button>
							<button
								class="pc-btn pc-btn-primary pc-btn-sm"
								@click="acceptShare(share)"
							>
								Accept
							</button>
							<button
								class="pc-btn pc-btn-secondary pc-btn-sm"
								@click="rejectShare(share)"
							>
								Dismiss
							</button>
						</div>
					</div>
				</div>
			</section>

			<!-- recent transfers -->
			<section class="mt-14" aria-label="Recent transfers" aria-live="polite">
				<div class="mb-3 flex items-center justify-between">
					<h3 class="text-xl font-bold tracking-tight">Recent transfers</h3>
					<button
						class="pc-btn pc-btn-ghost pc-btn-sm"
						@click="emit('navigate', 'notifications')"
					>
						View all
					</button>
				</div>
				<div v-if="recent.length" class="space-y-2">
					<div
						v-for="t in recent"
						:key="t.id + t.filename"
						class="pc-trow"
						:data-state="t.error ? 'failed' : t.done ? 'done' : 'active'"
					>
						<div class="pc-inset flex h-11 w-11 shrink-0 items-center justify-center">
							<PcIcon
								:name="t.error ? 'alert' : t.done ? 'check' : transferIcon(t.filename)"
								:size="18"
								:class="[
									t.error ? 'text-danger' : t.done ? 'pc-donecheck' : 'text-muted',
									t.done && !t.error ? 'pc-drawcheck' : '',
								]"
							/>
						</div>
						<div class="min-w-0 flex-1">
							<p class="truncate text-sm font-semibold">{{ t.filename }}</p>
							<p class="mt-0.5 truncate text-xs text-muted">
								{{ formatBytes(t.total) }} · {{ store.deviceName(t.deviceId) }}
								<span v-if="!t.done && !t.error && t.total > 0">
									· {{ toPercent(t.bytes / t.total) }}%
								</span>
							</p>
							<div v-if="!t.done && !t.error && t.total > 0" class="mt-2">
								<ProgressWave :value="t.bytes / t.total" :height="18" />
							</div>
							<p v-if="t.error" class="mt-1 text-xs text-danger">{{ t.error }}</p>
						</div>
						<div class="flex shrink-0 items-center gap-2">
							<template v-if="!t.done && !t.error">
								<button
									type="button"
									class="pc-btn pc-btn-ghost pc-btn-sm"
									:aria-label="`Cancel ${t.filename}`"
									@click="store.cancelTransfer(t)"
								>
									Cancel
								</button>
							</template>
							<template v-else-if="t.error">
								<button
									type="button"
									class="pc-btn pc-btn-secondary pc-btn-sm"
									@click="retryTransfer(t)"
								>
									Retry
								</button>
								<button
									type="button"
									class="pc-btn pc-btn-ghost pc-btn-sm"
									:aria-label="`Dismiss ${t.filename}`"
									@click="dismissTransfer(t)"
								>
									Dismiss
								</button>
							</template>
							<template v-else>
								<div class="relative">
									<button
										type="button"
										class="pc-btn pc-btn-ghost pc-btn-sm"
										:aria-expanded="openMenuId === t.id + t.filename"
										:aria-label="`More actions for ${t.filename}`"
										@click="toggleMenu(t.id + t.filename)"
										@keydown="closeMenuOnEsc"
									>
										<PcIcon name="menu" :size="16" />
									</button>
									<div
										v-if="openMenuId === t.id + t.filename"
										class="pc-menu absolute right-0 z-20 w-56"
										role="menu"
										@keydown="closeMenuOnEsc"
									>
										<button
											type="button"
											class="pc-menu__item"
											role="menuitem"
											@click="copyTransferName(t)"
										>
											<PcIcon name="copy" :size="14" />
											Copy file name
										</button>
										<button
											type="button"
											class="pc-menu__item"
											role="menuitem"
											disabled
											title="Not available: the backend reports no on-disk path for transfers"
										>
											<PcIcon name="file" :size="14" />
											Open
										</button>
										<button
											type="button"
											class="pc-menu__item"
											role="menuitem"
											disabled
											title="Not available: the backend reports no on-disk path for transfers"
										>
											<PcIcon name="folder" :size="14" />
											Show in folder
										</button>
									</div>
								</div>
							</template>
						</div>
					</div>
				</div>
				<div v-else class="pc-card flex flex-col items-center px-6 py-10 text-center">
					<PcIcon name="upload" :size="28" class="text-faint" />
					<p class="mt-3 font-semibold">No transfers yet</p>
					<p class="mt-1 max-w-md text-sm text-muted">
						Send text or a file above and it will appear here with its progress.
					</p>
				</div>
			</section>

			<!-- floating connected-device pill -->
			<Transition
				enter-active-class="transition duration-300 ease-emphasized"
				enter-from-class="opacity-0 translate-y-4"
				leave-active-class="transition duration-200"
				leave-to-class="opacity-0"
			>
				<button
					v-if="connectedDevice"
					type="button"
					class="pc-card fixed bottom-8 left-1/2 z-20 flex -translate-x-1/2 items-center gap-3 px-5 py-3 shadow-2xl"
					@click="emit('navigate', 'devices')"
				>
					<span class="pc-status-dot bg-online" />
					<div class="text-left">
						<p class="text-sm font-semibold leading-tight">{{ connectedDevice.name }}</p>
						<p class="text-xs text-muted">
							{{ connectedDevice.address ?? 'Connected' }}
						</p>
					</div>
					<PcIcon name="wifi" :size="18" class="ml-2 text-muted" />
				</button>
			</Transition>
		</div>
	</div>
</template>

<style scoped>
.pc-hero {
	background-color: rgb(var(--pc-card));
	border-radius: var(--pc-radius-card);
	border: 1px solid rgb(var(--pc-line));
	padding: 1.25rem 1.5rem;
}
.pc-hero[data-state='online'] {
	border-color: rgb(var(--pc-online) / 0.4);
}
.pc-battery__text {
	fill: rgb(var(--pc-text));
	font-size: 15px;
	font-weight: 800;
	font-variant-numeric: tabular-nums;
}

/* Wavy indicator modes: flat when offline, gentle breathing when connected,
   visibly travelling while a transfer runs, flat again once it completes. */
.pc-wavebox[data-mode='flat'] {
	opacity: 0.45;
}
.pc-wavebox[data-mode='idle'] svg {
	animation: pc-wave-breathe 4s ease-in-out infinite;
}
.pc-wavebox[data-mode='live'] {
	position: relative;
}
.pc-wavebox[data-mode='live'] svg {
	animation: pc-wave-travel 1.6s ease-in-out infinite;
}
@keyframes pc-wave-breathe {
	0%,
	100% {
		opacity: 1;
	}
	50% {
		opacity: 0.72;
	}
}
@keyframes pc-wave-travel {
	0%,
	100% {
		transform: translateX(-3px);
	}
	50% {
		transform: translateX(3px);
	}
}

/* Filled send-text area with a floating label. */
.pc-float {
	position: relative;
	display: flex;
}
.pc-float__input {
	width: 100%;
	resize: vertical;
	background-color: rgb(var(--pc-card-alt));
	border-radius: var(--pc-radius-field);
	padding: 1.5rem 1rem 0.75rem;
	font-size: 1rem;
	line-height: 1.5rem;
	color: rgb(var(--pc-text));
	outline: none;
}
.pc-float__input:focus {
	outline: 2px solid var(--pc-primary);
	outline-offset: 1px;
}
.pc-float__label {
	position: absolute;
	left: 1rem;
	top: 1rem;
	font-size: 0.875rem;
	color: rgb(var(--pc-faint));
	pointer-events: none;
	transition:
		top var(--pc-duration-short) var(--pc-ease-emphasized),
		font-size var(--pc-duration-short) var(--pc-ease-emphasized),
		color var(--pc-duration-short) var(--pc-ease-emphasized);
}
.pc-float__input:focus + .pc-float__label,
.pc-float__input:not(:placeholder-shown) + .pc-float__label {
	top: 0.45rem;
	font-size: 0.8125rem;
	color: rgb(var(--pc-muted));
}

/* Dropzone: animated dashed border + accent tint + bouncing icon while over. */
.pc-dropzone[data-active='true'] {
	border-color: var(--pc-primary);
	background-color: rgb(var(--pc-accent) / 0.12);
	animation: pc-drop-march 1s linear infinite;
}
.pc-dropzone[data-active='true'] .pc-dropicon {
	animation: pc-drop-bounce 0.6s cubic-bezier(0.3, 1.4, 0.5, 1) infinite;
	background-color: rgb(var(--pc-accent));
	color: rgb(var(--pc-on-accent));
}
@keyframes pc-drop-march {
	0% {
		filter: brightness(1);
	}
	50% {
		filter: brightness(1.18);
	}
	100% {
		filter: brightness(1);
	}
}
@keyframes pc-drop-bounce {
	0%,
	100% {
		transform: translateY(0) scale(1);
	}
	50% {
		transform: translateY(-6px) scale(1.06);
	}
}

.pc-filechip {
	display: flex;
	align-items: center;
	gap: 0.75rem;
	border-radius: var(--pc-radius-inner);
	background-color: rgb(var(--pc-card-alt));
	padding: 0.625rem 0.5rem 0.625rem 0.875rem;
	animation: pc-chip-in var(--pc-duration-short) cubic-bezier(0.3, 1.4, 0.5, 1);
}
@keyframes pc-chip-in {
	0% {
		opacity: 0;
		transform: scale(0.96) translateY(4px);
	}
	100% {
		opacity: 1;
		transform: none;
	}
}

/* Recent-transfer rows. */
.pc-trow {
	display: flex;
	align-items: center;
	gap: 0.875rem;
	border-radius: var(--pc-radius-inner);
	background-color: rgb(var(--pc-card-alt));
	padding: 0.75rem 0.75rem 0.75rem 1rem;
}
.pc-trow[data-state='active'] {
	outline: 1px solid rgb(var(--pc-line));
}
.pc-donecheck {
	color: rgb(var(--pc-online));
}
/* Self-drawing check once a transfer completes. */
.pc-drawcheck {
	stroke-dasharray: 32;
	stroke-dashoffset: 32;
	animation: pc-draw 500ms var(--pc-ease-emphasized) forwards;
}
@keyframes pc-draw {
	to {
		stroke-dashoffset: 0;
	}
}

div :focus-visible,
button:focus-visible,
textarea:focus-visible {
	outline: 2px solid var(--pc-primary);
	outline-offset: 2px;
}
</style>
