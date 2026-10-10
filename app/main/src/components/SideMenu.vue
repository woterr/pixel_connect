<script setup lang="ts">
// Pixel Connect — application sidebar.
//
// Reproduces the left rail from the supplied designs: wordmark with an eyebrow
// subtitle, pill navigation, a prominent "Share File" action, and Help/Exit at
// the foot. On narrow layouts the rail collapses into a slide-over.

import { computed } from 'vue'

import PcIcon from './atoms/PcIcon.vue'
import { useAppStore } from '../stores/app'
import { quitApp } from '../api'

export type ViewName = 'files' | 'devices' | 'notifications' | 'settings'

const props = defineProps<{
	view: ViewName
	open: boolean
}>()

const emit = defineEmits<{ navigate: [ViewName]; close: [] }>()

const store = useAppStore()

const items: { key: ViewName; label: string; icon: string; badge?: number }[] = [
	{ key: 'files', label: 'Files', icon: 'folder' },
	{ key: 'devices', label: 'Devices', icon: 'phone' },
	{ key: 'notifications', label: 'Notifications', icon: 'bell' },
	{ key: 'settings', label: 'Settings', icon: 'settings' },
]

const notificationCount = computed(() => store.notifications.length)

function go(key: ViewName) {
	emit('navigate', key)
	emit('close')
}
</script>

<template>
	<!-- Scrim for the mobile drawer -->
	<div
		v-if="open"
		class="fixed inset-0 z-30 bg-black/60 lg:hidden"
		aria-hidden="true"
		@click="emit('close')"
	/>

	<aside
		class="fixed inset-y-0 left-0 z-40 flex w-[320px] flex-col border-r border-ink-line px-5 py-6 transition-transform duration-200 ease-emphasized lg:static lg:z-auto lg:translate-x-0"
		:class="open ? 'translate-x-0' : '-translate-x-full'"
		aria-label="Main navigation"
	>
		<div class="flex items-start justify-between">
			<div>
				<h1 class="text-2xl font-extrabold italic tracking-tight">Pixel Connect</h1>
				<p class="pc-eyebrow mt-1">Local network</p>
			</div>
			<button
				type="button"
				class="pc-btn pc-btn-ghost -mr-2 h-10 w-10 p-0 lg:hidden"
				aria-label="Close navigation"
				@click="emit('close')"
			>
				<PcIcon name="close" />
			</button>
		</div>

		<nav class="mt-8 flex flex-col gap-1" aria-label="Sections">
			<button
				v-for="item in items"
				:key="item.key"
				type="button"
				class="pc-nav-item"
				:aria-current="view === item.key ? 'page' : undefined"
				@click="go(item.key)"
			>
				<PcIcon :name="item.icon" :size="18" />
				<span>{{ item.label }}</span>
				<span
					v-if="item.key === 'notifications' && notificationCount > 0"
					class="ml-auto rounded-full bg-current/20 px-2 py-0.5 text-xs font-semibold tabular-nums"
				>
					{{ notificationCount }}
				</span>
			</button>
		</nav>

		<div class="mt-6">
			<button
				type="button"
				class="pc-btn pc-btn-primary w-full py-3 text-sm"
				@click="go('files')"
			>
				<PcIcon name="share" :size="18" />
				<span>Share File</span>
			</button>
		</div>

		<div class="mt-auto space-y-1 pt-6">
			<button
				type="button"
				class="pc-nav-item"
				@click="store.toast('Documentation lives in docs/ inside the Pixel Connect project.', 'info')"
			>
				<PcIcon name="help" :size="18" />
				<span>Help</span>
			</button>
			<button type="button" class="pc-nav-item" @click="quitApp()">
				<PcIcon name="bolt" :size="18" />
				<span>Exit</span>
			</button>
		</div>
	</aside>
</template>