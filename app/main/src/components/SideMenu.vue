<script setup lang="ts">
// Pixel Connect — application sidebar.
//
// Material 3 Expressive navigation: a 280px panel at >=1000px carrying the
// brand, a live device-status card, pill navigation with a sliding indicator,
// a Share-File extended FAB (the only filled-primary control here) and quiet
// Help/Exit actions. Below 1000px the panel becomes an 80px icon rail with a
// modal drawer for the full panel. Navigation state comes from the store; no
// status is fabricated.

import { computed, nextTick, ref, watch } from 'vue'

import BrandMark from './atoms/BrandMark.vue'
import Dialog from './atoms/Dialog.vue'
import Disclosure from './atoms/Disclosure.vue'
import PcIcon from './atoms/PcIcon.vue'
import { useAppStore } from '../stores/app'
import { quitApp } from '../api'

export type ViewName = 'files' | 'devices' | 'notifications' | 'settings'

const props = defineProps<{
	view: ViewName
	open: boolean
}>()

const emit = defineEmits<{ navigate: [ViewName]; close: []; openDrawer: [] }>()

const store = useAppStore()

const items: { key: ViewName; label: string; icon: string }[] = [
	{ key: 'files', label: 'Files', icon: 'folder' },
	{ key: 'devices', label: 'Devices', icon: 'phone' },
	{ key: 'notifications', label: 'Notifications', icon: 'bell' },
	{ key: 'settings', label: 'Settings', icon: 'settings' },
]

const notificationCount = computed(() => store.notifications.length)
const activeIndex = computed(() => Math.max(0, items.findIndex((i) => i.key === props.view)))

const device = computed(() => store.preferredDevice)
const deviceStatusWord = computed(() => {
	const d = device.value
	if (!d) return 'No phone paired yet'
	if (d.linkState === 'Connected') return 'Connected'
	if (d.linkState === 'Connecting' || d.linkState === 'PairingRequested') return 'Connecting…'
	if (d.linkState === 'PairingRequestedByPeer') return 'Pairing requested'
	return 'Not reachable'
})
const deviceOnline = computed(() => device.value?.linkState === 'Connected')

function go(key: ViewName) {
	emit('navigate', key)
	emit('close')
}

/** Arrow-key travel across the nav buttons; Esc closes the modal drawer. */
const navRefs = ref<HTMLButtonElement[]>([])
function setNavRef(el: HTMLButtonElement | null, idx: number) {
	if (el) navRefs.value[idx] = el
}
function onNavKey(e: KeyboardEvent) {
	const idx = navRefs.value.findIndex((el) => el === document.activeElement)
	if (e.key === 'ArrowDown' || (e.key === 'ArrowRight' && idx === -1)) {
		e.preventDefault()
		const next = idx === -1 ? activeIndex.value : (idx + 1) % items.length
		navRefs.value[next]?.focus()
	} else if (e.key === 'ArrowUp' || e.key === 'ArrowLeft') {
		e.preventDefault()
		const prev = idx <= 0 ? items.length - 1 : idx - 1
		navRefs.value[prev]?.focus()
	} else if (e.key === 'Escape') {
		emit('close')
	}
}

const closeBtn = ref<HTMLButtonElement | null>(null)
watch(
	() => props.open,
	(open) => {
		if (open) nextTick(() => closeBtn.value?.focus())
	},
)

const helpOpen = ref(false)

function onHelp() {
	helpOpen.value = true
}
</script>

<template>
	<!-- Scrim for the modal drawer -->
	<Transition name="pc-scrim">
		<div
			v-if="open"
			class="pc-scrim fixed inset-0 z-30 bg-black/60"
			aria-hidden="true"
			@click="emit('close')"
		/>
	</Transition>

	<!-- Modal drawer: the full panel as an overlay below 1000px (and on demand above) -->
	<Transition name="pc-drawer">
		<aside
			v-if="open"
			class="pc-panel fixed inset-y-0 left-0 z-40 flex w-[280px] flex-col px-5 py-6"
			aria-label="Main navigation"
			@keydown="onNavKey"
		>
			<div class="flex items-center gap-3">
				<BrandMark :size="36" />
				<div class="min-w-0 flex-1">
					<p class="truncate text-lg font-extrabold italic tracking-tight">Pixel Connect</p>
				</div>
				<button
					ref="closeBtn"
					type="button"
					class="pc-btn pc-btn-ghost h-10 w-10 shrink-0 p-0"
					aria-label="Close navigation"
					@click="emit('close')"
				>
					<PcIcon name="close" />
				</button>
			</div>

			<button
				type="button"
				class="pc-device-card mt-6 text-left"
				@click="go('devices')"
			>
				<span class="pc-status-dot shrink-0" :class="deviceOnline ? 'bg-online' : 'bg-faint'" aria-hidden="true" />
				<span class="min-w-0 flex-1">
					<span class="block truncate text-sm font-semibold">{{ device?.name ?? 'No phone yet' }}</span>
					<span class="block truncate text-xs text-muted">{{ deviceStatusWord }}</span>
				</span>
				<PcIcon name="phone" :size="18" class="shrink-0 text-muted" />
			</button>

			<nav class="pc-nav mt-6 flex flex-col gap-1" aria-label="Sections" @keydown="onNavKey">
				<span
					class="pc-nav-indicator"
					aria-hidden="true"
					:style="{ transform: `translateY(${activeIndex * 52}px)` }"
				/>
				<button
					v-for="(item, i) in items"
					:key="item.key"
					:ref="(el) => setNavRef(el as HTMLButtonElement | null, i)"
					type="button"
					class="pc-nav-btn"
					:aria-current="view === item.key ? 'page' : undefined"
					:aria-label="item.key === 'notifications' && notificationCount > 0 ? `Notifications, ${notificationCount} unread` : undefined"
					@click="go(item.key)"
				>
					<span class="pc-nav-icon" :data-active="view === item.key">
						<PcIcon :name="item.icon" :size="20" />
					</span>
					<span class="pc-nav-label">{{ item.label }}</span>
					<span
						v-if="item.key === 'notifications' && notificationCount > 0"
						:key="notificationCount"
						class="pc-badge ml-auto"
					>
						{{ notificationCount > 99 ? '99+' : notificationCount }}
					</span>
				</button>
			</nav>

			<div class="mt-6">
				<button type="button" class="pc-fab" @click="go('files')">
					<PcIcon name="share" :size="20" />
					<span>Share File</span>
				</button>
			</div>

			<div class="mt-auto space-y-1 pt-6">
				<button type="button" class="pc-quiet" @click="onHelp">
					<PcIcon name="help" :size="18" />
					<span>Help</span>
				</button>
				<button type="button" class="pc-quiet" @click="quitApp()">
					<PcIcon name="bolt" :size="18" />
					<span>Exit</span>
				</button>
			</div>
		</aside>
	</Transition>

	<!-- Persistent rail/panel -->
	<aside class="pc-panel pc-rail flex flex-col px-3 py-5" aria-label="Main navigation">
		<div class="pc-rail-wide-only items-center gap-3 px-2">
			<BrandMark :size="36" />
			<p class="min-w-0 flex-1 truncate text-lg font-extrabold italic tracking-tight">Pixel Connect</p>
		</div>
		<div class="pc-rail-narrow-only flex-col items-center gap-3">
			<button
				type="button"
				class="pc-btn pc-btn-ghost h-10 w-10 p-0"
				aria-label="Open navigation"
				@click="emit('openDrawer')"
			>
				<PcIcon name="menu" />
			</button>
			<BrandMark :size="32" />
		</div>

		<button
			type="button"
			class="pc-device-card pc-rail-wide-only mt-6 text-left"
			@click="go('devices')"
		>
			<span class="pc-status-dot shrink-0" :class="deviceOnline ? 'bg-online' : 'bg-faint'" aria-hidden="true" />
			<span class="min-w-0 flex-1">
				<span class="block truncate text-sm font-semibold">{{ device?.name ?? 'No phone yet' }}</span>
				<span class="block truncate text-xs text-muted">{{ deviceStatusWord }}</span>
			</span>
			<PcIcon name="phone" :size="18" class="shrink-0 text-muted" />
		</button>
		<button
			type="button"
			class="pc-rail-narrow-only mt-4 h-10 w-10 items-center justify-center"
			:aria-label="device ? `${device.name}, ${deviceStatusWord}. Open Devices.` : 'No phone yet. Open Devices.'"
			@click="go('devices')"
		>
			<span class="pc-status-dot" :class="deviceOnline ? 'bg-online' : 'bg-faint'" aria-hidden="true" />
		</button>

		<nav class="pc-nav mt-6 flex flex-col gap-1" aria-label="Sections" @keydown="onNavKey">
			<span
				class="pc-nav-indicator"
				aria-hidden="true"
				:style="{ transform: `translateY(${activeIndex * 52}px)` }"
			/>
			<button
				v-for="(item, i) in items"
				:key="item.key"
				:ref="(el) => setNavRef(el as HTMLButtonElement | null, i)"
				type="button"
				class="pc-nav-btn pc-rail-btn"
				:aria-current="view === item.key ? 'page' : undefined"
				:title="item.label"
				:aria-label="item.key === 'notifications' && notificationCount > 0 ? `${item.label}, ${notificationCount} unread` : item.label"
				@click="go(item.key)"
			>
				<span class="pc-nav-icon" :data-active="view === item.key">
					<PcIcon :name="item.icon" :size="20" />
				</span>
				<span class="pc-nav-label pc-rail-wide-only">{{ item.label }}</span>
				<span
					v-if="item.key === 'notifications' && notificationCount > 0"
					:key="notificationCount"
					class="pc-badge pc-rail-wide-only ml-auto"
				>
					{{ notificationCount > 99 ? '99+' : notificationCount }}
				</span>
				<span
					v-if="item.key === 'notifications' && notificationCount > 0"
					:key="`dot-${notificationCount}`"
					class="pc-badge-dot pc-rail-narrow-only"
					aria-hidden="true"
				/>
			</button>
		</nav>

		<div class="mt-6">
			<button type="button" class="pc-fab pc-rail-wide-only" @click="go('files')">
				<PcIcon name="share" :size="20" />
				<span>Share File</span>
			</button>
			<button
				type="button"
				class="pc-fab-icon pc-rail-narrow-only"
				aria-label="Share File"
				title="Share File"
				@click="go('files')"
			>
				<PcIcon name="share" :size="20" />
			</button>
		</div>

		<div class="mt-auto flex flex-col gap-1 pt-6">
			<button type="button" class="pc-quiet pc-rail-btn" title="Help" aria-label="Help" @click="onHelp">
				<PcIcon name="help" :size="18" />
				<span class="pc-rail-wide-only">Help</span>
			</button>
			<button type="button" class="pc-quiet pc-rail-btn" title="Exit" aria-label="Exit" @click="quitApp()">
				<PcIcon name="bolt" :size="18" />
				<span class="pc-rail-wide-only">Exit</span>
			</button>
		</div>
	</aside>

	<!-- Help & troubleshooting. All troubleshooting guidance lives here so a
		stuck user has one place to look instead of hunting across screens. -->
	<Dialog :open="helpOpen" title="Help & troubleshooting" @close="helpOpen = false" @update:open="helpOpen = $event">
		<div class="flex flex-col gap-3 text-sm">
			<p class="text-muted">
				Pixel Connect links this computer to the phone over the local network
				(KDE Connect) and over Bluetooth (Quick Share). Both need the devices
				to reach each other; neither needs an account or the internet.
			</p>
			<Disclosure label="Phone isn't found or won't stay connected">
				<ul class="list-disc space-y-1 pl-5 text-muted">
					<li>Put the phone on the same Wi-Fi network as this computer.</li>
					<li>KDE Connect on the phone runs as a background service — the app does not need to be open, but the service must be enabled.</li>
					<li>Press Reconnect on the Files screen, then look under Devices for a Pair button and confirm the code on both sides.</li>
					<li>If the phone claims it is connected but this app shows nothing, the phone is talking to something else (e.g. a previous install): unpair it on the phone and pair again here.</li>
					<li>Some routers isolate Wi-Fi clients from each other; a phone hotspot or a different access point is worth trying.</li>
				</ul>
			</Disclosure>
			<Disclosure label="Clipboard sync doesn't arrive">
				<ul class="list-disc space-y-1 pl-5 text-muted">
					<li>Clipboard sync needs a live link. Copying while the phone is idle first wakes it, which takes a few seconds — copy again if the first attempt misses.</li>
					<li>Check Settings → Preferences: both clipboard switches must be on.</li>
					<li>On some Wayland compositors the window must be focused to read the clipboard; the setting screen says what this session supports.</li>
				</ul>
			</Disclosure>
			<Disclosure label="Quick Share sticks at Connecting">
				<ul class="list-disc space-y-1 pl-5 text-muted">
					<li>On a shared network the phone uses Wi-Fi and transfers are quick. Over Bluetooth alone, discovery can take up to a minute and large files are slow (about 6&nbsp;KB/s) — small files and text are fine.</li>
					<li>If the accept prompt never appears, the offer expired: send again.</li>
					<li>With Auto-accept incoming shares on, offers are accepted immediately.</li>
				</ul>
			</Disclosure>
			<Disclosure label="Where to look when something fails">
				<ul class="list-disc space-y-1 pl-5 text-muted">
					<li>Notifications carry failure reasons; transfers can be retried from there.</li>
					<li>Settings → About shows versions and paths. Logs go to the journal when run as a service.</li>
				</ul>
			</Disclosure>
		</div>
	</Dialog>
</template>

<style scoped>
.pc-panel {
	background-color: rgb(var(--pc-raised));
	border-right: 1px solid rgb(var(--pc-line));
}
.pc-rail {
	width: 80px;
	align-items: stretch;
}
@media (min-width: 1000px) {
	.pc-rail {
		width: 280px;
		padding-inline: 1.25rem;
		padding-block: 1.5rem;
	}
}
.pc-rail-wide-only {
	display: none;
}
@media (min-width: 1000px) {
	.pc-rail-wide-only {
		display: flex;
	}
	.pc-rail-narrow-only {
		display: none !important;
	}
}
.pc-rail-narrow-only {
	display: flex;
}

/* Device status card: the live replacement for the old static caption. */
.pc-device-card {
	display: flex;
	align-items: center;
	gap: 0.625rem;
	padding: 0.625rem 0.75rem;
	border-radius: 1rem;
	background-color: rgb(var(--pc-card));
	border: 1px solid rgb(var(--pc-line));
	transition:
		background-color var(--pc-duration-short) var(--pc-ease-emphasized),
		border-color var(--pc-duration-short) var(--pc-ease-emphasized);
}
.pc-device-card:hover {
	background-color: rgb(var(--pc-card-alt));
}

/* Nav: 48px rows with one sliding pill behind the active row. */
.pc-nav {
	position: relative;
}
.pc-nav-indicator {
	position: absolute;
	left: 0;
	right: 0;
	top: 0;
	height: 48px;
	border-radius: 9999px;
	background-color: rgb(var(--pc-card-alt));
	transition: transform var(--pc-duration-medium) cubic-bezier(0.3, 1.2, 0.5, 1);
	pointer-events: none;
}
.pc-nav-btn {
	position: relative;
	display: flex;
	align-items: center;
	gap: 0.75rem;
	min-height: 48px;
	padding: 0 1rem;
	border-radius: 9999px;
	font-size: 0.9375rem;
	font-weight: 500;
	color: rgb(var(--pc-muted));
	transition: color var(--pc-duration-short) var(--pc-ease-emphasized);
}
.pc-nav-btn:hover {
	color: rgb(var(--pc-text));
}
.pc-nav-btn[aria-current='page'] {
	color: rgb(var(--pc-text));
	font-weight: 600;
}
.pc-rail-btn {
	justify-content: flex-start;
}
@media (max-width: 999.98px) {
	.pc-rail-btn {
		justify-content: center;
		padding: 0;
	}
}
/* Filled icon disc when selected. */
.pc-nav-icon {
	display: inline-flex;
	align-items: center;
	justify-content: center;
	width: 2rem;
	height: 2rem;
	border-radius: 9999px;
	flex: none;
	transition:
		background-color var(--pc-duration-short) var(--pc-ease-emphasized),
		color var(--pc-duration-short) var(--pc-ease-emphasized),
		transform var(--pc-duration-short) cubic-bezier(0.34, 1.3, 0.64, 1);
}
.pc-nav-icon[data-active='true'] {
	background-color: rgb(var(--pc-accent));
	color: rgb(var(--pc-on-accent));
	transform: scale(1.05);
}
.pc-nav-label {
	white-space: nowrap;
}

/* Animated notification badge. */
.pc-badge {
	display: inline-flex;
	align-items: center;
	justify-content: center;
	min-width: 1.375rem;
	height: 1.375rem;
	padding: 0 0.375rem;
	border-radius: 9999px;
	background-color: rgb(var(--pc-accent));
	color: rgb(var(--pc-on-accent));
	font-size: 0.8125rem;
	font-weight: 700;
	font-variant-numeric: tabular-nums;
	animation: pc-badge-pop var(--pc-duration-short) cubic-bezier(0.3, 1.4, 0.5, 1);
}
.pc-badge-dot {
	position: absolute;
	top: 6px;
	right: 14px;
	width: 0.625rem;
	height: 0.625rem;
	border-radius: 9999px;
	background-color: rgb(var(--pc-accent));
	animation: pc-badge-pop var(--pc-duration-short) cubic-bezier(0.3, 1.4, 0.5, 1);
}
@keyframes pc-badge-pop {
	0% {
		transform: scale(0.4);
	}
	100% {
		transform: scale(1);
	}
}

/* Extended FAB: the only filled-primary control in the sidebar. */
.pc-fab {
	display: inline-flex;
	align-items: center;
	justify-content: center;
	gap: 0.625rem;
	width: 100%;
	padding: 1rem 1.5rem;
	border-radius: 1.25rem;
	background-color: rgb(var(--pc-accent));
	color: rgb(var(--pc-on-accent));
	font-weight: 700;
	font-size: 0.9375rem;
	box-shadow:
		0 8px 24px -8px rgb(var(--pc-accent) / 0.55),
		0 2px 6px rgb(0 0 0 / 0.3);
	transition:
		transform var(--pc-duration-short) var(--pc-ease-emphasized),
		filter var(--pc-duration-short) var(--pc-ease-emphasized);
}
.pc-fab:hover {
	filter: brightness(1.08);
}
.pc-fab:active {
	transform: scale(0.97);
}
.pc-fab-icon {
	display: inline-flex;
	align-items: center;
	justify-content: center;
	width: 3.5rem;
	height: 3.5rem;
	margin-inline: auto;
	border-radius: 1.25rem;
	background-color: rgb(var(--pc-accent));
	color: rgb(var(--pc-on-accent));
	box-shadow:
		0 8px 24px -8px rgb(var(--pc-accent) / 0.55),
		0 2px 6px rgb(0 0 0 / 0.3);
	transition: transform var(--pc-duration-short) var(--pc-ease-emphasized);
}
.pc-fab-icon:active {
	transform: scale(0.94);
}

/* Quiet footer actions. */
.pc-quiet {
	display: flex;
	align-items: center;
	gap: 0.75rem;
	width: 100%;
	min-height: 44px;
	padding: 0.5rem 1rem;
	border-radius: 9999px;
	font-size: 0.9375rem;
	font-weight: 500;
	color: rgb(var(--pc-muted));
	transition:
		background-color var(--pc-duration-short) var(--pc-ease-emphasized),
		color var(--pc-duration-short) var(--pc-ease-emphasized);
}
.pc-quiet:hover {
	color: rgb(var(--pc-text));
	background-color: rgb(var(--pc-card-alt));
}
@media (max-width: 999.98px) {
	.pc-quiet {
		justify-content: center;
		padding: 0.5rem 0;
	}
}

.pc-panel :focus-visible,
.pc-rail :focus-visible {
	outline: 2px solid var(--pc-primary);
	outline-offset: 2px;
}

.pc-scrim-enter-active,
.pc-scrim-leave-active {
	transition: opacity var(--pc-duration-short) var(--pc-ease-emphasized);
}
.pc-scrim-enter-from,
.pc-scrim-leave-to {
	opacity: 0;
}
.pc-drawer-enter-active {
	transition: transform var(--pc-duration-medium) var(--pc-ease-decelerate);
}
.pc-drawer-leave-active {
	transition: transform var(--pc-duration-short) var(--pc-ease-accelerate);
}
.pc-drawer-enter-from,
.pc-drawer-leave-to {
	transform: translateX(-100%);
}
</style>
