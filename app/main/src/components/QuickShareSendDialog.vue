<script setup lang="ts">
// Pixel Connect — Quick Share send dialog.
//
// A formal menu for sending over Quick Share: the selected file up top, live
// discovery state in the middle (scanning / targets / honest empty hint), one
// row per discovered endpoint with its transport. Replaces an earlier inline
// section that only appeared when targets happened to exist, which read as a
// missing feature whenever the phone was not discoverable at that moment.

import { computed, ref } from 'vue'

import Dialog from './atoms/Dialog.vue'
import PcIcon from './atoms/PcIcon.vue'
import { useAppStore } from '../stores/app'
import * as api from '../api'

const props = defineProps<{
	open: boolean
	filename: string
	filepath: string
}>()

const emit = defineEmits<{ close: [] }>()

const store = useAppStore()
const sending = ref<string | null>(null)
const scanning = ref(false)

const targets = computed(() =>
	store.quickshareEndpoints.map((ep) => ({
		key: ep.id ?? ep.address ?? ep.name ?? 'endpoint',
		id: ep.id ?? ep.address ?? '',
		name: ep.name ?? ep.address ?? 'Nearby device',
		ble: (ep.id ?? '').startsWith('ble://') || !ep.address,
	})),
)

/** Restart discovery so a phone that just opened its share sheet appears. */
async function rescan() {
	scanning.value = true
	try {
		await api.quickshareStopDiscovery()
		await api.quickshareStartDiscovery()
	} catch (e) {
		store.toast(`Could not restart discovery: ${e}`, 'error')
	} finally {
		scanning.value = false
	}
}

async function sendTo(target: { key: string; id: string; name: string }) {
	if (!target.id) {
		store.toast('That target has no usable address yet.', 'error')
		return
	}
	sending.value = target.key
	try {
		await api.quickshareSendFiles(target.id, target.name, [props.filepath])
		store.toast(`Sharing ${props.filename} with ${target.name}…`, 'info')
		emit('close')
	} catch (e) {
		store.toast(`Could not share via Quick Share: ${e}`, 'error')
	} finally {
		sending.value = null
	}
}
</script>

<template>
	<Dialog :open="props.open" title="Share via Quick Share" @close="emit('close')" @update:open="emit('close')">
		<div class="flex flex-col gap-4">
			<div class="pc-inset flex items-center gap-3 px-4 py-3">
				<PcIcon name="file" :size="20" class="shrink-0 text-muted" />
				<p class="min-w-0 flex-1 truncate text-sm font-semibold">{{ props.filename }}</p>
			</div>

			<div class="flex items-center justify-between gap-3">
				<p class="pc-eyebrow">Nearby targets</p>
				<button
					type="button"
					class="pc-btn pc-btn-ghost pc-btn-sm"
					:disabled="scanning"
					@click="rescan"
				>
					<PcIcon name="refresh" :size="14" :class="scanning ? 'animate-spin' : ''" />
					{{ scanning ? 'Scanning…' : 'Rescan' }}
				</button>
			</div>

			<div v-if="targets.length" class="flex flex-col gap-2">
				<button
					v-for="t in targets"
					:key="t.key"
					type="button"
					class="pc-card pc-card-interactive flex w-full items-center gap-3 px-4 py-3 text-left"
					:disabled="sending !== null"
					@click="sendTo(t)"
				>
					<PcIcon :name="t.ble ? 'bluetooth' : 'wifi'" :size="18" class="shrink-0 text-muted" />
					<span class="min-w-0 flex-1">
						<span class="block truncate font-semibold">{{ t.name }}</span>
						<span class="block text-xs text-muted">
							{{ t.ble ? 'Bluetooth' : 'Wi-Fi' }}
							{{ sending === t.key ? '· sending…' : '' }}
						</span>
					</span>
				</button>
			</div>
			<div v-else class="rounded-2xl bg-inset px-4 py-5 text-sm text-muted">
				<p class="font-semibold text-ink">No targets found yet</p>
				<p class="mt-1">
					Open Quick Share on the phone and choose it as if to receive — this
					machine appears once the phone starts advertising. Then press Rescan.
				</p>
			</div>

			<p class="text-xs text-faint">
				Transfers use Wi-Fi when the phone allows it, Bluetooth otherwise.
			</p>
		</div>
	</Dialog>
</template>
