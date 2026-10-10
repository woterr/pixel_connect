<script setup lang="ts">
// Pixel Connect — Notifications view.
//
// Implements the "PixelOS Notifications" shade concept from the supplied
// designs: a system-canvas column of notification cards, with an active-transfer
// card on top (wave progress plus pause/cancel affordances) and received-item
// cards below that can be opened or saved.

import { computed } from 'vue'

import PcIcon from './atoms/PcIcon.vue'
import ProgressWave from './atoms/ProgressWave.vue'
import { useAppStore } from '../stores/app'
import type { TransferProgress } from '../types'
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

async function runAction(key: string, action: string) {
	const deviceId = key.split(':')[0]
	try {
		await api.kdeSendNotificationAction(deviceId, key, action)
		store.toast(`Sent "${actionLabel(action)}" to the phone`, 'success')
	} catch (e) {
		store.toast(`Could not send the action: ${e}`, 'error')
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

				<div class="mt-5 flex gap-2">
					<button class="pc-btn pc-btn-secondary pc-btn-sm" disabled>
						Pause
					</button>
					<button class="pc-btn pc-btn-secondary pc-btn-sm" disabled>
						Cancel
					</button>
					<span class="pc-chip self-center">Pause/Cancel arrive with the protocol extension</span>
				</div>
			</article>

			<!-- phone notifications -->
			<article
				v-for="n in store.notifications"
				:key="n.key"
				class="pc-card mt-4 p-5"
			>
				<div class="flex items-center gap-3">
					<div class="pc-inset flex h-10 w-10 items-center justify-center rounded-full">
						<PcIcon name="bell" :size="16" class="text-muted" />
					</div>
					<div class="min-w-0 flex-1">
						<p class="truncate font-semibold">{{ n.title || n.appName }}</p>
						<p class="truncate text-sm text-muted">{{ n.text }}</p>
					</div>
					<span class="shrink-0 text-xs text-faint">{{ timeAgo(n.timestamp) }}</span>
				</div>
				<div v-if="n.actions.length" class="mt-4 flex flex-wrap gap-2">
					<button
						v-for="a in n.actions"
						:key="a"
						class="pc-btn pc-btn-secondary pc-btn-sm"
						@click="runAction(n.key, a)"
					>
						{{ actionLabel(a) }}
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