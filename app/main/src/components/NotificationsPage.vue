<script setup lang="ts">
// Pixel Connect — Notifications view.
//
// Implements the "PixelOS Notifications" shade concept from the supplied
// designs: a system-canvas column of notification cards, with an active-transfer
// card on top (wave progress plus pause/cancel affordances) and received-item
// cards below that can be opened or saved.

import { computed, reactive, ref } from 'vue'

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

/** Invoke an action the phone offered on this notification. */
async function runAction(n: RemoteNotification, action: string) {
	const deviceId = n.id.split(':')[0] ?? ''
	if (!deviceId) return
	busy.value = `${n.id}:${action}`
	try {
		await api.kdeSendNotificationAction(deviceId, n.id, action)
		store.toast(`Sent “${actionLabel(action)}” to the phone`, 'success')
		if (action === 'cancel') {
			store.notifications = store.notifications.filter((x) => x.id !== n.id)
		}
	} catch (e) {
		store.toast(`Could not send the action: ${e}`, 'error')
	} finally {
		busy.value = null
	}
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
		store.notifications = store.notifications.filter((x) => x.id !== notificationId)
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
	await api.kdeClearNotifications()
	store.notifications = []
	store.toast('Notifications cleared', 'success')
}

function cardIcon(name: string | null | undefined) {
	const cat = fileCategory(name)
	return cat === 'image' ? 'image' : 'file'
}
</script>

<template>
	<div class="pc-scroll flex-1 px-6 pb-28 pt-6 sm:px-10 lg:px-14 lg:pb-12">
		<div class="mx-auto max-w-3xl">
			<header class="flex items-start justify-between">
				<div>
					<p class="pc-eyebrow">System shade concept</p>
					<h2 class="pc-display mt-2 text-4xl sm:text-5xl">PixelOS<br />Notifications</h2>
				</div>
				<button class="pc-btn pc-btn-ghost h-11 w-11 p-0" aria-label="Notifications">
					<PcIcon name="bell" :size="20" />
				</button>
			</header>

			<!-- active transfer -->
			<article
				v-if="active"
				class="pc-card mt-10 border-l-4 border-l-paper p-6 dark:border-l-paper"
			>
				<div class="flex items-center gap-4">
					<div class="pc-inset flex h-11 w-11 items-center justify-center rounded-full">
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

			<!-- phone notifications -->
			<article v-for="n in store.notifications" :key="n.id" class="pc-card mt-4 p-5">
				<div class="flex items-center gap-3">
					<div class="pc-inset flex h-10 w-10 items-center justify-center rounded-full">
						<PcIcon name="bell" :size="16" class="text-muted" />
					</div>
					<div class="min-w-0 flex-1">
						<p class="truncate font-semibold">
							{{ n.title || n.appName }}
							<span v-if="n.silent" class="ml-1 text-faint">(silent)</span>
						</p>
						<p class="truncate text-sm text-muted">{{ n.text }}</p>
						<p class="mt-0.5 truncate text-xs text-faint">
							{{ n.appName }} · {{ store.deviceName(n.id.split(':')[0] ?? '', 'Phone') }} ·
							{{ timeAgo(n.timestamp) }}
						</p>
					</div>
				</div>

				<!-- typed reply: only when the phone marked it repliable -->
				<form
					v-if="n.requestReplyId"
					class="mt-4 flex gap-2"
					@submit.prevent="sendReply(n.id, replyDraft[n.id] ?? '')"
				>
					<input
						v-model="replyDraft[n.id]"
						type="text"
						class="pc-inset flex-1 bg-transparent px-4 py-2 text-sm outline-none"
						placeholder="Reply from this computer…"
						:aria-label="`Reply to ${n.appName}`"
						maxlength="4096"
					/>
					<button
						type="submit"
						class="pc-btn pc-btn-primary pc-btn-sm"
						:disabled="!(replyDraft[n.id] ?? '').trim()"
					>
						<PcIcon name="send" :size="14" />
						Send
					</button>
				</form>

				<div v-if="n.actions.length || n.isClearable" class="mt-4 flex flex-wrap gap-2">
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
						v-if="n.isClearable"
						class="pc-btn pc-btn-ghost pc-btn-sm"
						:disabled="busy === `${n.id}:cancel`"
						@click="runAction(n, 'cancel')"
					>
						Dismiss
					</button>
				</div>
			</article>

			<!-- received items -->
			<article v-for="item in received" :key="'recv-' + item.filename" class="pc-card mt-4 p-5">
				<div class="flex items-center gap-3">
					<div class="pc-inset flex h-10 w-10 items-center justify-center rounded-full">
						<PcIcon name="download" :size="16" class="text-muted" />
					</div>
					<div class="min-w-0 flex-1">
						<p class="truncate font-semibold">Received Item</p>
						<p class="truncate text-sm text-muted">
							From {{ store.devices.find((d: import('../types').KcDevice) => d.id === item.deviceId)
							?.name ?? 'Device' }}
						</p>
					</div>
					<span class="shrink-0 text-xs text-faint">Now</span>
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

			<!-- empty state -->
			<div
				v-if="!active && !store.notifications.length && !received.length"
				class="mt-16 text-center"
			>
				<div class="mx-auto h-1.5 w-16 rounded-full bg-ink-line" />
				<p class="pc-eyebrow mt-6">System canvas</p>
				<p class="mt-3 text-muted">
					Nothing yet. Notifications from the phone and your transfers appear here.
				</p>
				<button class="pc-btn pc-btn-secondary pc-btn-md mt-6" @click="clearAll">
					Refresh
				</button>
			</div>

			<button
				v-if="store.notifications.length"
				class="pc-btn pc-btn-secondary pc-btn-md mt-8 w-full"
				@click="clearAll"
			>
				Clear all
			</button>
		</div>
	</div>
</template>