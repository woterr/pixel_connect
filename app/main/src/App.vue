<script setup lang="ts">
// Pixel Connect — application shell.
//
// Wires the sidebar, the routed views, the toast stack and the backend event
// subscriptions together. All application state comes from the store; nothing
// is fabricated in the UI.

import { onBeforeUnmount, onMounted, ref } from 'vue'

import DevicesPage from './components/DevicesPage.vue'
import FilesPage from './components/FilesPage.vue'
import NotificationsPage from './components/NotificationsPage.vue'
import SettingsPage from './components/SettingsPage.vue'
import SideMenu, { type ViewName } from './components/SideMenu.vue'
import PcIcon from './components/atoms/PcIcon.vue'
import ToastMessage from './components/atoms/ToastMessage.vue'
import { useAppStore } from './stores/app'
import type { EndpointInfo, KcDevice } from './types'
import * as api from './api'

const store = useAppStore()
const view = ref<ViewName>('files')
const drawerOpen = ref(false)
let unlisteners: Array<() => void> = []

async function boot() {
	await store.bootstrap()

	const sub = async (p: Promise<() => void>) => {
		const fn = await p
		unlisteners.push(fn)
	}

	await sub(api.on('kdeconnect:device', (d: KcDevice) => store.upsertDevice(d)))
	await sub(
		api.on('kdeconnect:pairing_request', (d) => {
			store.upsertDevice(d)
			store.toast(
				`${d.name} wants to pair. Confirm code ${d.verificationKey ?? ''} on both devices.`,
				'info',
			)
		}),
	)
	await sub(
		api.on('kdeconnect:clipboard', ({ deviceName }) => {
			store.toast(`Clipboard updated from ${deviceName}`, 'info')
		}),
	)
	await sub(api.on('kdeconnect:share', (s) => store.shares.push(s)))
	await sub(api.on('kdeconnect:notification', (n) => store.notifications.unshift(n)))
	await sub(api.on('kdeconnect:transfer', (t) => store.applyTransfer(t)))
	await sub(api.on('quickshare:event', (m) => store.applyQuickShareEvent(m)))
	await sub(
		api.on('quickshare:endpoint', (e) => {
			const i = store.quickshareEndpoints.findIndex(
				(x: EndpointInfo) => x.address === e.address,
			)
			if (i === -1) store.quickshareEndpoints.push(e)
			else store.quickshareEndpoints[i] = e
		}),
	)

	// Quick Share discovery: keep scanning so the Files page can send.
	try {
		await api.quickshareStartDiscovery()
	} catch (e) {
		store.toast(`Quick Share discovery could not start: ${e}`, 'error')
	}

	// Periodic device/ADB refresh keeps the UI honest about real state.
	//
	// `kdeRefresh` asks the backend to dial every peer it has discovered.
	// Reading `kdeDevices` alone only re-reads the list, so without this a
	// phone that appears on the network is discovered but never connected, and
	// therefore never appears in the list at all.
	const timer = setInterval(() => {
		api
			.kdeRefresh()
			.catch(() => undefined)
			.finally(() => store.refreshAll().catch(() => undefined))
	}, 6000)
	unlisteners.push(() => clearInterval(timer))

	// Clipboard -> phone.
	//
	// On most sessions the backend watcher already runs in the background and
	// this is unnecessary. On a compositor with neither a data-control protocol
	// nor an XWayland bridge, only a focused window may read the clipboard, so
	// the front end drives detection: poll while focused, and catch up
	// immediately whenever the window regains focus.
	if (store.clipboardCaps && !store.clipboardCaps.backgroundReadAllowed) {
		let lastSeen = ''
		const syncNow = async () => {
			if (document.hasFocus()) return
			try {
				const text = await api.clipboardRead()
				if (!text || text === lastSeen) return
				lastSeen = text
				await pushClipboard(text)
			} catch {
				/* clipboard access can be denied; that is the expected case here */
			}
		}
		void syncNow()
		const onFocus = () => void syncNow()
		window.addEventListener('focus', onFocus)
		const clipTimer = setInterval(syncNow, 1500)
		unlisteners.push(() => {
			window.removeEventListener('focus', onFocus)
			clearInterval(clipTimer)
		})
	}
}

/** Forward locally-observed clipboard text to the paired phone. */
async function pushClipboard(text: string) {
	if (!store.settings?.clipboardAutoShare) return
	const target = store.preferredDevice
	if (!target || target.linkState !== 'Connected') return
	try {
		await api.kdeShareText(target.id, text)
		store.toast(`Clipboard sent to ${target.name}`, 'success')
	} catch (e) {
		store.toast(`Could not send the clipboard: ${e}`, 'error')
	}
}

function navigate(next: ViewName) {
	view.value = next
	drawerOpen.value = false
}

onMounted(boot)

// Ask the backend to dial everything it has already discovered, once, as soon
// as the application starts. Without this the first device list stays empty
// until the periodic poll above happens to fire.
api.kdeRefresh().catch(() => undefined)
onBeforeUnmount(() => {
	for (const fn of unlisteners) {
		try {
			fn()
		} catch {
			/* ignore teardown races */
		}
	}
	unlisteners = []
})
</script>

<template>
	<div class="flex h-full w-full overflow-hidden bg-ink text-ink dark:bg-ink dark:text-paper">
		<SideMenu :view="view" :open="drawerOpen" @navigate="navigate" @close="drawerOpen = false" />

		<div class="relative flex min-w-0 flex-1 flex-col">
			<!-- compact top bar for narrow layouts -->
			<header
				class="flex items-center justify-between border-b border-ink-line px-5 py-3 lg:hidden"
			>
				<button
					type="button"
					class="pc-btn pc-btn-ghost h-10 w-10 p-0"
					aria-label="Open navigation"
					@click="drawerOpen = true"
				>
					<PcIcon name="menu" />
				</button>
				<h1 class="text-lg font-extrabold italic tracking-tight">Pixel Connect</h1>
				<button
					type="button"
					class="pc-btn pc-btn-ghost h-10 w-10 p-0"
					aria-label="Notifications"
					@click="navigate('notifications')"
				>
					<PcIcon name="bell" :size="18" />
				</button>
			</header>

			<FilesPage v-if="view === 'files'" :view="view" @navigate="navigate" />
			<DevicesPage v-else-if="view === 'devices'" />
			<NotificationsPage v-else-if="view === 'notifications'" />
			<SettingsPage v-else />
		</div>

		<!-- toasts -->
		<div class="pointer-events-none fixed bottom-4 right-4 z-50 flex w-80 flex-col gap-2">
			<TransitionGroup
				enter-active-class="transition duration-200 ease-emphasized"
				enter-from-class="opacity-0 translate-y-2"
				leave-active-class="transition duration-150"
				leave-to-class="opacity-0"
			>
				<ToastMessage
					v-for="t in store.toasts"
					:key="t.id"
					:id="t.id"
					:kind="t.kind"
					:message="t.message"
					@dismiss="store.dismissToast"
				/>
			</TransitionGroup>
		</div>
	</div>
</template>