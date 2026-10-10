<script setup lang="ts">
// Pixel Connect — Files view.
//
// Reproduces the primary screen from the supplied designs:
//   * a status row with an ONLINE / OFFLINE pill and two utility actions
//   * the huge transfer headline ("Receiving files…") with a percentage and the
//     signature wavy progress rule
//   * an active-file card with a cancel affordance
//   * two side-by-side cards: Send Text and Send File (drag & drop dropzone)
//   * a "Recent Activity" strip and the floating connected-device pill
//   * the bottom navigation on narrow layouts

import { computed, onBeforeUnmount, ref, watch } from 'vue'

import PcIcon from './atoms/PcIcon.vue'
import ProgressWave from './atoms/ProgressWave.vue'
import type { ViewName } from './SideMenu.vue'
import type { PendingShare } from '../types'
import { useAppStore } from '../stores/app'
import { formatBytes, formatEta, formatRate, toPercent, etaFrom, fileCategory } from '../utils'
import * as api from '../api'

const emit = defineEmits<{ navigate: [ViewName] }>()
const props = defineProps<{ view: ViewName }>()

const store = useAppStore()

const dragActive = ref(false)
const textDraft = ref('')
const sendingText = ref(false)
const selectedFile = ref<{ name: string; path: string; size: number } | null>(null)

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

async function openPendingShare(id: string) {
	await api.kdeDismissShare(id)
	emit('navigate', 'devices')
}

// ---- activity ------------------------------------------------------------

const activity = computed(() =>
	store.transfers
		.slice()
		.reverse()
		.slice(0, 5),
)

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
		<!-- status row -->
		<div class="flex items-center justify-between">
			<div
				class="flex items-center gap-2 text-xs font-semibold tracking-wide"
				:class="store.isOnline ? 'text-online' : 'text-faint'"
			>
				<span class="pc-status-dot" :class="store.isOnline ? 'bg-online' : 'bg-faint'" />
				{{ store.isOnline ? 'ONLINE' : 'OFFLINE' }}
			</div>
			<div class="flex items-center gap-2">
				<button
					type="button"
					class="pc-btn pc-btn-ghost h-10 w-10 p-0"
					aria-label="Notifications"
					@click="emit('navigate', 'notifications')"
				>
					<PcIcon name="bell" :size="18" />
				</button>
				<button
					type="button"
					class="pc-btn pc-btn-ghost h-10 w-10 p-0"
					aria-label="Settings"
					@click="emit('navigate', 'settings')"
				>
					<PcIcon name="settings" :size="18" />
				</button>
			</div>
		</div>

		<!-- headline + wave -->
		<section class="mt-10">
			<div class="flex items-baseline justify-between gap-6">
				<h2 class="pc-display text-4xl sm:text-5xl lg:text-6xl">{{ headline }}</h2>
				<span
					v-if="percentLabel"
					class="shrink-0 text-3xl font-bold tabular-nums sm:text-4xl"
				>
					{{ percentLabel }}
				</span>
			</div>
			<ProgressWave class="mt-6" :value="progress" />

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
			</div>
		</section>

		<!-- send text + send file -->
		<section class="mt-14 grid gap-6 lg:grid-cols-2">
			<!-- Send Text -->
			<div class="pc-card flex flex-col p-6">
				<div class="flex items-center gap-2">
					<PcIcon name="send" :size="18" class="text-muted" />
					<h3 class="font-semibold">Send Text</h3>
				</div>
				<textarea
					v-model="textDraft"
					class="mt-4 min-h-[130px] flex-1 resize-none rounded-xl bg-transparent text-lg outline-none placeholder:text-faint"
					placeholder="Type or paste content to share…"
					aria-label="Text to share"
					@keydown.meta.enter="sendText"
					@keydown.ctrl.enter="sendText"
				/>
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
					:class="dragActive ? '' : ''"
					:data-active="dragActive"
					@dragenter.prevent="dragActive = true"
					@dragover.prevent="dragActive = true"
					@dragleave.prevent="dragActive = false"
					@drop.prevent="onDrop"
				>
					<div class="pc-inset flex h-12 w-12 items-center justify-center rounded-full">
						<PcIcon name="plus" :size="22" />
					</div>
					<p class="text-sm text-muted">
						Drag &amp; drop or
						<button class="underline" @click="browseForFile">Browse</button>
					</p>
				</div>
				<div class="mt-4 rounded-xl bg-ink-card-alt px-4 py-3 text-center text-sm text-muted">
					<template v-if="selectedFile">
						{{ selectedFile.name }} · {{ formatBytes(selectedFile.size) }}
					</template>
					<template v-else> No file selected yet </template>
				</div>
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
		<section v-if="store.shares.length" class="mt-14">
			<h3 class="pc-eyebrow mb-3">Incoming shares</h3>
			<div class="space-y-3">
				<div
					v-for="share in store.shares"
					:key="share.id"
					class="pc-card flex items-center gap-4 px-5 py-4"
				>
					<div class="pc-inset flex h-12 w-12 items-center justify-center">
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
					<button
						v-if="share.text || share.url"
						class="pc-btn pc-btn-primary pc-btn-sm"
						@click="copySharedText(share)"
					>
						Copy to clipboard
					</button>
					<button
						v-else
						class="pc-btn pc-btn-secondary pc-btn-sm"
						@click="openPendingShare(share.id)"
					>
						Review
					</button>
				</div>
			</div>
		</section>

		<!-- recent activity -->
		<section class="mt-14">
			<div class="mb-3 flex items-center justify-between">
				<h3 class="text-xl font-bold tracking-tight">Recent Activity</h3>
				<button
					class="pc-btn pc-btn-ghost pc-btn-sm"
					@click="emit('navigate', 'notifications')"
				>
					View All
				</button>
			</div>
			<div v-if="activity.length" class="space-y-2">
				<div
					v-for="t in activity"
					:key="t.id + t.filename"
					class="pc-inset flex items-center gap-3 px-4 py-3 text-sm"
				>
					<PcIcon
						:name="t.error ? 'alert' : t.done ? 'check' : 'upload'"
						:size="16"
						:class="t.error ? 'text-danger' : t.done ? 'text-online' : 'text-muted'"
					/>
					<span class="min-w-0 flex-1 truncate">{{ t.filename }}</span>
					<span class="shrink-0 text-muted tabular-nums">
						{{ formatBytes(t.bytes) }} / {{ formatBytes(t.total) }}
					</span>
				</div>
			</div>
			<p v-else class="text-sm text-faint">No activity yet.</p>
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
				class="pc-card fixed bottom-24 left-1/2 z-20 flex -translate-x-1/2 items-center gap-3 px-5 py-3 shadow-2xl lg:bottom-8"
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

		<!-- mobile bottom nav -->
		<nav
			class="fixed inset-x-0 bottom-0 z-30 flex items-center justify-around border-t border-ink-line bg-ink px-6 py-3 lg:hidden"
			aria-label="Primary"
		>
			<button
				class="pc-btn pc-btn-primary px-6 py-2 text-sm"
				:aria-current="props.view === 'files' ? 'page' : undefined"
				@click="emit('navigate', 'files')"
			>
				<PcIcon name="folder" :size="18" />
				Files
			</button>
			<button
				class="pc-btn pc-btn-ghost px-6 py-2 text-sm"
				@click="emit('navigate', 'settings')"
			>
				<PcIcon name="settings" :size="18" />
				Settings
			</button>
		</nav>
	</div>
</template>