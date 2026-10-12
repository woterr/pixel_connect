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
import BrandMark from './components/atoms/BrandMark.vue'
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
	// Apply the stored accent and motion choices alongside the theme. The
	// store's applyTheme covers data-theme; these two live with it so the
	// whole appearance lands before anything renders.
	applyAppearance()
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
			// Only a focused window may read the clipboard on this compositor,
			// so skip when unfocused rather than when focused. The previous
			// code had this inverted (`if (document.hasFocus()) return`),
			// which meant the frontend never synced exactly when it was the
			// only path that could work.
			if (!document.hasFocus()) return
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

/**
 * Mirror the stored accent / reduce-motion choices onto the document, next to
 * what `store.applyTheme` does for the theme. Unknown accents fall back to
 * the default rather than being applied blind, matching the backend's
 * validation. Each write is cached for the pre-paint contract in tokens.ts.
 */
function applyAppearance() {
	if (typeof document === 'undefined') return
	const accent = store.settings?.accent
	if (accent) document.documentElement.setAttribute('data-accent', accent)
	const reduce = store.settings?.reduceMotion === true
	if (reduce) document.documentElement.setAttribute('data-reduce-motion', '')
	else document.documentElement.removeAttribute('data-reduce-motion')
	try {
		if (store.settings?.theme) localStorage.setItem('pc-theme', store.settings.theme)
		if (accent) localStorage.setItem('pc-accent', accent)
		localStorage.setItem('pc-reduce-motion', reduce ? '1' : '0')
	} catch {
		/* private mode: styling still applies for this session */
	}
}

/** Forward locally-observed clipboard text to the paired phone. */
async function pushClipboard(text: string) {
	if (!store.settings?.clipboardAutoShare) return
	const target = store.preferredDevice
	if (!target) return
	// Do not silently drop when the link is down: the backend watcher dials
	// via wake_trusted_devices before sending, but this frontend path sent
	// directly and returned early on anything but Connected, so a copy made
	// while the phone was idle was lost. Refresh first (which dials
	// discovered peers), then attempt the send and let the backend error
	// surface instead of dropping silently.
	if (target.linkState !== 'Connected') {
		try {
			await api.kdeRefresh()
			await store.refreshAll()
		} catch {
			/* refresh is best-effort; the send below reports the outcome */
		}
	}
	try {
		// Re-resolve after refresh: the device list may have changed.
		const fresh =
			store.devices.find((d) => d.id === target.id) ?? store.preferredDevice
		if (!fresh) {
			store.toast('No paired Pixel is available. Pair one under Devices first.', 'error')
			return
		}
		await api.kdeShareText(fresh.id, text)
		store.toast(`Clipboard sent to ${fresh.name}`, 'success')
	} catch (e) {
		store.toast(`Could not send the clipboard: ${e}`, 'error')
	}
}

function navigate(next: ViewName) {
	view.value = next
	drawerOpen.value = false
}

// ---- window controls ------------------------------------------------------
//
// The OS frame stays in charge (decorations are native): these buttons invoke
// the same Tauri window the frame buttons drive, so Close still honours the
// backend's realclose setting (hide-to-tray vs quit) and nothing about window
// behaviour changes. Outside Tauri the cluster hides itself.

interface WindowControls {
	minimize: () => void
	toggleMaximize: () => void
	close: () => void
	isMaximized: () => Promise<boolean>
}

const windowControls = ref<WindowControls | null>(null)
const maximized = ref(false)

async function refreshMaximized() {
	try {
		if (windowControls.value) maximized.value = await windowControls.value.isMaximized()
	} catch {
		/* best-effort chrome state */
	}
}

onMounted(async () => {
	try {
		const { getCurrentWindow } = await import('@tauri-apps/api/window')
		const w = getCurrentWindow()
		windowControls.value = {
			minimize: () => void w.minimize().catch(() => undefined),
			toggleMaximize: () => void w.toggleMaximize().catch(() => undefined).finally(() => void refreshMaximized()),
			close: () => void w.close().catch(() => undefined),
			isMaximized: () => w.isMaximized(),
		}
		await refreshMaximized()
	} catch {
		windowControls.value = null
	}
})

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
	<div class="flex h-full w-full flex-col overflow-hidden bg-ink text-ink">
		<!-- Title bar: mark + title + the window cluster driving the real window. -->
		<header
			class="pc-titlebar flex h-12 shrink-0 items-center gap-3 border-b border-ink-line px-4"
			data-tauri-drag-region
		>
			<BrandMark :size="24" />
			<p class="min-w-0 flex-1 truncate text-sm font-bold italic tracking-tight">
				Pixel Connect
			</p>
			<div
				v-if="windowControls"
				class="flex shrink-0 items-center gap-1"
				role="group"
				aria-label="Window"
			>
				<button
					type="button"
					class="pc-winbtn"
					aria-label="Minimize"
					title="Minimize"
					@click="windowControls.minimize()"
				>
					<span aria-hidden="true" class="pc-winbtn__glyph">–</span>
				</button>
				<button
					type="button"
					class="pc-winbtn"
					:aria-label="maximized ? 'Restore' : 'Maximize'"
					:title="maximized ? 'Restore' : 'Maximize'"
					@click="windowControls.toggleMaximize()"
				>
					<span aria-hidden="true" class="pc-winbtn__glyph">{{ maximized ? '❐' : '▢' }}</span>
				</button>
				<button
					type="button"
					class="pc-winbtn pc-winbtn--close"
					aria-label="Close"
					title="Close"
					@click="windowControls.close()"
				>
					<PcIcon name="close" :size="14" />
				</button>
			</div>
		</header>

		<div class="flex min-h-0 w-full flex-1">
			<SideMenu
				:view="view"
				:open="drawerOpen"
				@navigate="navigate"
				@close="drawerOpen = false"
				@open-drawer="drawerOpen = true"
			/>

			<!-- Shared-axis-Y fade-through between pages: 28px over 300ms. -->
			<main class="relative flex min-w-0 flex-1 flex-col" aria-live="off">
				<Transition name="pc-page" mode="out-in">
					<FilesPage
						v-if="view === 'files'"
						:key="'files'"
						:view="view"
						@navigate="navigate"
					/>
					<DevicesPage v-else-if="view === 'devices'" :key="'devices'" />
					<NotificationsPage v-else-if="view === 'notifications'" :key="'notifications'" />
					<SettingsPage v-else :key="'settings'" />
				</Transition>
			</main>
		</div>

		<!-- toasts -->
		<div class="pointer-events-none fixed bottom-4 right-4 z-50 flex w-80 max-w-[calc(100vw-2rem)] flex-col gap-2">
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

<style scoped>
.pc-titlebar {
	background-color: rgb(var(--pc-raised));
	-webkit-user-select: none;
	user-select: none;
}
.pc-winbtn {
	display: inline-flex;
	align-items: center;
	justify-content: center;
	width: 2.5rem;
	height: 2rem;
	border-radius: 0.5rem;
	color: rgb(var(--pc-muted));
	transition:
		background-color var(--pc-duration-shortest) ease,
		color var(--pc-duration-shortest) ease;
	-webkit-app-region: no-drag;
}
.pc-winbtn:hover {
	background-color: rgb(var(--pc-card-alt));
	color: rgb(var(--pc-text));
}
.pc-winbtn--close:hover {
	background-color: rgb(var(--pc-danger));
	color: #ffffff;
}
.pc-winbtn__glyph {
	font-size: 1rem;
	line-height: 1;
	font-weight: 600;
}
.pc-winbtn:focus-visible {
	outline: 2px solid var(--pc-primary);
	outline-offset: 1px;
}

/* Shared-axis-Y fade-through: rise 28px and fade in over 300ms emphasized. */
.pc-page-enter-active {
	transition:
		opacity var(--pc-duration-medium) var(--pc-ease-emphasized),
		transform var(--pc-duration-medium) var(--pc-ease-emphasized);
}
.pc-page-leave-active {
	transition:
		opacity 200ms var(--pc-ease-accelerate),
		transform 200ms var(--pc-ease-accelerate);
}
.pc-page-enter-from {
	opacity: 0;
	transform: translateY(28px);
}
.pc-page-leave-to {
	opacity: 0;
	transform: translateY(-24px);
}
</style>
