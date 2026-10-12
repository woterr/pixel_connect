<script setup lang="ts">
// Pixel Connect — Notifications view.
//
// Phone notifications grouped by device and app, the in-flight transfer card
// and received items. Cards always show a heading and a body through honest
// fallback chains (title -> text -> app name; text -> title -> "No preview"),
// so a notification with empty fields never renders as a bare bell with
// metadata. Dismissals and Clear-all both offer Undo.

import { computed, reactive, ref, watch } from 'vue'

import PcIcon from './atoms/PcIcon.vue'
import ProgressWave from './atoms/ProgressWave.vue'
import { useAppStore } from '../stores/app'
import type { RemoteNotification, TransferProgress } from '../types'
import * as api from '../api'
import { formatBytes, fileCategory, timeAgo, toPercent } from '../utils'

const store = useAppStore()

const active = computed<import('../types').TransferProgress | null>(
	() => store.activeTransfers[0] ?? null,
)

const received = computed(() =>
	store.transfers.filter((t: TransferProgress) => t.done).slice().reverse(),
)

const actionLabel = (a: string): string =>
	({ pause: 'Pause', cancel: 'Cancel', open: 'Open', reply: 'Reply' })[a] ?? a

const busy = ref<string | null>(null)
/** Draft reply text, keyed by notification id. */
const replyDraft = reactive<Record<string, string>>({})
/** Reply boxes stay collapsed until tapped. */
const showReply = reactive<Record<string, boolean>>({})

/** Honest heading: title, then text, then the app name, never empty. */
function headingFor(n: RemoteNotification): string {
	return n.title || n.text || n.appName || 'Notification'
}

/** Honest body: text, then title, then an explicit lack of preview. */
function bodyFor(n: RemoteNotification): string {
	return n.text || n.title || 'No preview'
}

/** First letter avatar for the app, or a null when there is nothing to show. */
function appInitial(n: RemoteNotification): string | null {
	const s = (n.appName || '').trim()
	return s ? s[0]!.toUpperCase() : null
}

/** Notifications grouped by device, then by app, preserving arrival order. */
const groups = computed(() => {
	const byDevice = new Map<string, { deviceId: string; deviceName: string; apps: Map<string, RemoteNotification[]> }>()
	for (const n of store.notifications) {
		// A notification without an id cannot be grouped or acted on, but it
		// must not crash the whole view. The backend always sends one; the
		// visual-check stub once did not, which is how this was found.
		const deviceId = (n.id ?? '').split(':')[0] ?? ''
		let g = byDevice.get(deviceId)
		if (!g) {
			g = { deviceId, deviceName: store.deviceName(deviceId, 'Phone'), apps: new Map() }
			byDevice.set(deviceId, g)
		}
		const appKey = n.appName || 'Other'
		let list = g.apps.get(appKey)
		if (!list) {
			list = []
			g.apps.set(appKey, list)
		}
		list.push(n)
	}
	return [...byDevice.values()].map((g) => ({ ...g, apps: [...g.apps.entries()] }))
})

/** Announce newly arrived notifications to screen readers. */
const liveMessage = ref('')
let seenIds = new Set<string>()
watch(
	() => store.notifications.map((n) => n.id).join('\u0000'),
	() => {
		const current = store.notifications
		const fresh = current.filter((n) => !seenIds.has(n.id))
		seenIds = new Set(current.map((n) => n.id))
		if (fresh.length > 0) {
			const first = fresh[0]!
			liveMessage.value = `New notification from ${first.appName || 'the phone'}: ${headingFor(first)}`
		}
	},
	{ immediate: true },
)

// ---- undo snackbar -----------------------------------------------------------

interface RemovedEntry {
	item: RemoteNotification
	index: number
}

const undo = ref<{ entries: RemovedEntry[]; label: string } | null>(null)
let undoTimer: ReturnType<typeof setTimeout> | null = null

function showUndo(entries: RemovedEntry[], label: string) {
	undo.value = { entries, label }
	if (undoTimer) clearTimeout(undoTimer)
	undoTimer = setTimeout(() => {
		undo.value = null
		undoTimer = null
	}, 6000)
}

function doUndo() {
	if (!undo.value) return
	// Re-insert where each item was. Undo only restores this list: a dismissal
	// already confirmed by the phone stays dismissed there.
	const sorted = [...undo.value.entries].sort((a, b) => a.index - b.index)
	for (const e of sorted) {
		if (!store.notifications.some((x) => x.id === e.item.id)) {
			store.notifications.splice(Math.min(e.index, store.notifications.length), 0, e.item)
		}
	}
	undo.value = null
	if (undoTimer) {
		clearTimeout(undoTimer)
		undoTimer = null
	}
}

/** Invoke an action the phone offered on this notification. */
async function runAction(n: RemoteNotification, action: string) {
	const deviceId = (n.id ?? '').split(':')[0] ?? ''
	if (!deviceId) return
	busy.value = `${n.id}:${action}`
	try {
		await api.kdeSendNotificationAction(deviceId, n.id, action)
		store.toast(`Sent “${actionLabel(action)}” to the phone`, 'success')
		if (action === 'cancel') {
			removeWithUndo(n, 'Notification dismissed')
		}
	} catch (e) {
		store.toast(`Could not send the action: ${e}`, 'error')
	} finally {
		busy.value = null
	}
}

/** Remove a notification locally with an Undo offer. */
function removeWithUndo(n: RemoteNotification, label: string) {
	const index = store.notifications.findIndex((x) => x.id === n.id)
	if (index === -1) return
	store.notifications.splice(index, 1)
	showUndo([{ item: n, index }], label)
}

/** Send a typed reply; the backend resolves the phone's own reply id. */
async function sendReply(notificationId: string, text: string) {
	const body = text.trim()
	if (!body) return
	busy.value = `${notificationId}:reply`
	try {
		await api.kdeReplyToNotification(notificationId, body)
		store.toast('Reply sent to the phone', 'success')
		delete replyDraft[notificationId]
		delete showReply[notificationId]
		const n = store.notifications.find((x) => x.id === notificationId)
		if (n) removeWithUndo(n, 'Reply sent')
	} catch (e) {
		store.toast(`Could not send the reply: ${e}`, 'error')
	} finally {
		busy.value = null
	}
}

/** Cancel an in-flight transfer. */
async function cancel(t: TransferProgress) {
	busy.value = `${t.id}:cancel`
	try {
		await store.cancelTransfer(t)
	} finally {
		busy.value = null
	}
}

async function clearAll() {
	const entries = store.notifications.map((item, index) => ({ item, index }))
	try {
		await api.kdeClearNotifications()
	} catch (e) {
		store.toast(`Could not clear the notifications: ${e}`, 'error')
		return
	}
	store.notifications = []
	if (entries.length) showUndo(entries, `${entries.length === 1 ? 'Notification' : 'Notifications'} cleared`)
	else store.toast('Notifications cleared', 'success')
}

/** Live link state for a group header dot: the single source of truth is
 * the device record's linkState, exactly as the sidebar and Files hero read it. */
function groupOnline(deviceId: string): boolean {
	return store.devices.some((d) => d.id === deviceId && d.linkState === 'Connected')
}

function cardIcon(name: string | null | undefined) {
	const cat = fileCategory(name)
	return cat === 'image' ? 'image' : 'file'
}
</script>

<template>
	<div class="pc-scroll flex-1 px-6 pb-28 pt-6 sm:px-10 lg:px-14 lg:pb-12">
		<div class="mx-auto w-full max-w-3xl">
			<p class="sr-only" aria-live="polite">{{ liveMessage }}</p>
			<header class="flex flex-wrap items-center gap-3">
				<div class="min-w-0 flex-1">
					<div class="flex items-center gap-3">
						<h2 class="pc-display text-4xl sm:text-5xl">Notifications</h2>
						<span
							v-if="store.notifications.length"
							class="pc-count"
							:aria-label="`${store.notifications.length} notifications`"
						>
							{{ store.notifications.length }}
						</span>
					</div>
					<p class="mt-2 text-sm text-muted">
						{{ store.notifications.length === 0
							? 'Mirrored from your connected phones.'
							: `${store.notifications.length} mirrored from your connected phones.` }}
					</p>
				</div>
				<button
					v-if="store.notifications.length"
					class="pc-btn pc-btn-secondary pc-btn-md shrink-0"
					@click="clearAll"
				>
					Clear all
				</button>
			</header>

			<!-- active transfer -->
			<article
				v-if="active"
				class="pc-card mt-10 border-l-4 border-l-paper p-6"
				aria-label="Active transfer"
			>
				<div class="flex items-center gap-4">
					<div class="pc-inset flex h-11 w-11 shrink-0 items-center justify-center rounded-full">
						<PcIcon name="refresh" :size="18" />
					</div>
					<div class="min-w-0 flex-1">
						<p class="font-semibold">Active Transfer</p>
						<p class="truncate text-sm text-muted">
							{{ active.filename }} ·
							{{ store.deviceName(active.deviceId) }}
						</p>
					</div>
					<span class="pc-status-dot bg-online" />
				</div>

				<div class="mt-5 flex items-center gap-4">
					<ProgressWave
						class="flex-1"
						:value="active.total ? active.bytes / active.total : 0"
					/>
					<span class="shrink-0 text-sm font-semibold tabular-nums">
						{{ toPercent(active.total ? active.bytes / active.total : 0) }}%
					</span>
				</div>

				<div class="mt-5 flex flex-wrap items-center gap-2">
					<button
						class="pc-btn pc-btn-secondary pc-btn-sm"
						:disabled="busy === `${active.id}:cancel`"
						@click="cancel(active)"
					>
						<PcIcon name="close" :size="14" />
						Cancel transfer
					</button>
					<!--
						The KDE Connect protocol has no pause packet: neither the
						desktop nor the Android implementation defines one, so a
						pause control could not do anything. Cancel works because
						both implementations treat a closed payload socket as a
						cancellation.
					-->
					<span class="pc-chip self-center">Pause is not part of the protocol</span>
				</div>
			</article>

			<!-- phone notifications, grouped by device and app -->
			<section v-for="group in groups" :key="group.deviceId" aria-label="Phone notifications">
				<h3 class="pc-grouphead">
					<!-- Driven by the live link, like every other status dot. This was
						hardcoded green, so a disconnected phone looked connected here
						while every other screen said otherwise. -->
					<span
						class="pc-status-dot shrink-0"
						:class="groupOnline(group.deviceId) ? 'bg-online' : 'bg-faint'"
						aria-hidden="true"
					/>
					{{ group.deviceName }}
					<span class="text-faint">· {{ group.apps.reduce((n, [, items]) => n + items.length, 0) }}</span>
				</h3>
				<template v-for="[appName, items] in group.apps" :key="appName">
					<p class="pc-eyebrow mt-5">{{ appName }}</p>
					<TransitionGroup name="pc-notif" tag="div" class="mt-2 space-y-3">
						<article v-for="n in items" :key="n.id" class="pc-card pc-notif-card p-5">
							<div class="flex items-center gap-3">
								<div class="pc-avatar" aria-hidden="true">
									<span v-if="appInitial(n)">{{ appInitial(n) }}</span>
									<PcIcon v-else name="bell" :size="16" class="text-muted" />
								</div>
								<div class="min-w-0 flex-1">
									<p class="truncate font-semibold">
										{{ headingFor(n) }}
										<span v-if="n.silent" class="ml-1 font-normal text-faint">(silent)</span>
									</p>
									<p v-if="bodyFor(n) !== headingFor(n)" class="mt-0.5 line-clamp-3 break-words text-sm text-muted">
										{{ bodyFor(n) }}
									</p>
									<p class="mt-0.5 truncate text-xs text-faint">
										{{ n.appName || 'Phone notification' }} · {{ group.deviceName }} · {{ timeAgo(n.timestamp) }}
									</p>
								</div>
							</div>

							<!-- typed reply: only when the phone marked it repliable -->
							<div v-if="n.requestReplyId && !showReply[n.id]" class="mt-4 flex flex-wrap gap-2">
								<button type="button" class="pc-btn pc-btn-secondary pc-btn-sm" @click="showReply[n.id] = true">Reply</button>
							</div>
							<form
								v-if="n.requestReplyId && showReply[n.id]"
								class="mt-4 flex gap-2"
								@submit.prevent="sendReply(n.id, replyDraft[n.id] ?? '')"
							>
								<input
									v-model="replyDraft[n.id]"
									type="text"
									class="pc-inset min-w-0 flex-1 bg-transparent px-4 py-2 text-sm outline-none"
									placeholder="Reply from this computer…"
									:aria-label="`Reply to ${n.appName || 'notification'}`"
									maxlength="4096"
								/>
								<button
									type="submit"
									class="pc-btn pc-btn-primary pc-btn-sm shrink-0"
									:disabled="!(replyDraft[n.id] ?? '').trim()"
								>
									<PcIcon name="send" :size="14" />
									Send
								</button>
							</form>

							<div class="mt-4 flex flex-wrap gap-2">
								<button
									v-for="a in n.actions"
									:key="a"
									class="pc-btn pc-btn-secondary pc-btn-sm"
									:disabled="busy === `${n.id}:${a}`"
									@click="runAction(n, a)"
								>
									{{ a }}
								</button>
								<button
									class="pc-btn pc-btn-ghost pc-btn-sm"
									:disabled="busy === `${n.id}:cancel`"
									:title="n.isClearable ? 'Ask the phone to dismiss this notification' : 'The phone marked this as not clearable — this asks it to dismiss anyway'"
									@click="runAction(n, 'cancel')"
								>
									Dismiss
								</button>
							</div>
						</article>
					</TransitionGroup>
				</template>
			</section>

			<!-- received items -->
			<section v-if="received.length" class="mt-10" aria-label="Received items">
				<h3 class="pc-eyebrow mb-3">Received</h3>
				<div class="space-y-3">
					<article v-for="item in received" :key="'recv-' + item.filename" class="pc-card p-5">
						<div class="flex items-center gap-3">
							<div class="pc-inset flex h-10 w-10 shrink-0 items-center justify-center rounded-full">
								<PcIcon name="download" :size="16" class="text-muted" />
							</div>
							<div class="min-w-0 flex-1">
								<p class="truncate font-semibold">{{ item.filename }}</p>
								<p class="truncate text-sm text-muted">
									From {{ store.devices.find((d: import('../types').KcDevice) => d.id === item.deviceId)
									?.name ?? 'Device' }} · {{ formatBytes(item.total) }}
								</p>
							</div>
							<span v-if="!item.error" class="pc-pill shrink-0" data-tone="online">Saved</span>
							<span v-else class="pc-pill shrink-0" data-tone="danger">Failed</span>
						</div>
						<div class="pc-inset mt-4 flex items-center gap-3 px-4 py-3">
							<PcIcon :name="cardIcon(item.filename)" :size="18" class="text-muted" />
							<div class="min-w-0 flex-1">
								<p class="truncate text-sm font-medium">{{ item.filename }}</p>
								<p class="text-xs text-muted">{{ formatBytes(item.total) }}</p>
							</div>
						</div>
						<p v-if="item.error" class="mt-3 text-sm text-danger">{{ item.error }}</p>
					</article>
				</div>
			</section>

			<!-- empty state -->
			<div
				v-if="!active && !store.notifications.length && !received.length"
				class="mt-16 text-center"
			>
				<div class="pc-inset mx-auto flex h-16 w-16 items-center justify-center rounded-full">
					<PcIcon name="bell" :size="26" class="text-faint" />
				</div>
				<p class="pc-headline mt-6">You're all caught up</p>
				<p class="mx-auto mt-2 max-w-sm text-sm text-muted">
					Nothing yet. Notifications from the phone and your transfers appear here.
				</p>
			</div>

			<!-- undo snackbar -->
			<Transition name="pc-snack">
				<div v-if="undo" class="pc-snack" role="status">
					<p class="flex-1 text-sm font-medium">{{ undo.label }}</p>
					<button type="button" class="pc-btn pc-btn-primary pc-btn-sm shrink-0" @click="doUndo">
						Undo
					</button>
				</div>
			</Transition>
		</div>
	</div>
</template>

<style scoped>
.pc-count {
	display: inline-flex;
	align-items: center;
	justify-content: center;
	min-width: 2rem;
	height: 2rem;
	padding: 0 0.5rem;
	border-radius: 9999px;
	background-color: rgb(var(--pc-accent));
	color: rgb(var(--pc-on-accent));
	font-weight: 800;
	font-size: 0.9375rem;
	font-variant-numeric: tabular-nums;
}

/* Sticky device group headers. */
.pc-grouphead {
	position: sticky;
	top: 0;
	z-index: 10;
	display: flex;
	align-items: center;
	gap: 0.625rem;
	margin-top: 2rem;
	padding: 0.625rem 0.25rem;
	background-color: rgb(var(--pc-bg));
	border-bottom: 1px solid rgb(var(--pc-line));
	font-weight: 700;
	font-size: 1rem;
}

/* App fallback avatar. */
.pc-avatar {
	display: inline-flex;
	align-items: center;
	justify-content: center;
	flex: none;
	width: 2.5rem;
	height: 2.5rem;
	border-radius: 9999px;
	background-color: rgb(var(--pc-card-alt));
	color: rgb(var(--pc-muted));
	font-weight: 800;
	font-size: 1rem;
}

.pc-notif-card {
	max-height: 32rem;
	overflow: hidden;
}

/* New items spring in; dismissals slide out and collapse over 250ms. */
.pc-notif-enter-active {
	transition:
		opacity var(--pc-duration-short) var(--pc-ease-decelerate),
		transform var(--pc-duration-short) cubic-bezier(0.3, 1.4, 0.5, 1);
}
.pc-notif-enter-from {
	opacity: 0;
	transform: translateY(-10px) scale(0.98);
}
.pc-notif-leave-active {
	transition:
		opacity 250ms var(--pc-ease-accelerate),
		transform 250ms var(--pc-ease-accelerate),
		max-height 250ms var(--pc-ease-accelerate),
		margin 250ms var(--pc-ease-accelerate),
		padding 250ms var(--pc-ease-accelerate);
}
.pc-notif-leave-to {
	opacity: 0;
	transform: translateX(32px);
	max-height: 0 !important;
	margin-top: 0 !important;
	margin-bottom: 0 !important;
	padding-top: 0 !important;
	padding-bottom: 0 !important;
}
.pc-notif-move {
	transition: transform var(--pc-duration-medium) var(--pc-ease-emphasized);
}

.pc-snack {
	position: fixed;
	bottom: 1.5rem;
	left: 50%;
	transform: translateX(-50%);
	z-index: 40;
	display: flex;
	align-items: center;
	gap: 1rem;
	min-width: min(24rem, calc(100vw - 3rem));
	padding: 0.75rem 0.75rem 0.75rem 1.25rem;
	border-radius: var(--pc-radius-inner);
	background-color: rgb(var(--pc-card-alt));
	border: 1px solid rgb(var(--pc-line));
	box-shadow: 0 12px 32px rgb(0 0 0 / 0.4);
}
.pc-snack-enter-active {
	transition:
		opacity var(--pc-duration-short) var(--pc-ease-decelerate),
		transform var(--pc-duration-short) cubic-bezier(0.3, 1.4, 0.5, 1);
}
.pc-snack-leave-active {
	transition:
		opacity var(--pc-duration-short) var(--pc-ease-accelerate),
		transform var(--pc-duration-short) var(--pc-ease-accelerate);
}
.pc-snack-enter-from,
.pc-snack-leave-to {
	opacity: 0;
	transform: translateX(-50%) translateY(12px);
}

article :focus-visible,
button:focus-visible,
input:focus-visible {
	outline: 2px solid var(--pc-primary);
	outline-offset: 2px;
}
</style>
