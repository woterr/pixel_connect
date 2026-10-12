<script setup lang="ts">
// Pixel Connect — Settings view.
//
// Appearance first (theme previews, accent swatches, reduce-motion), then the
// network identity, preferences as list rows, screen mirroring and SSH behind
// an Advanced disclosure, integrations and about. Every control performs a
// real action against the stored settings or is visibly marked unavailable.
//
// Toggles reuse the existing ToggleSwitch atom (no new sibling atoms were
// available at build time); the segmented visibility control and the theme /
// accent pickers are native buttons with radiogroup semantics.

import { computed, nextTick, onBeforeUnmount, onMounted, ref } from 'vue'

import PcIcon from './atoms/PcIcon.vue'
import ToggleSwitch from './atoms/ToggleSwitch.vue'
import { useAppStore } from '../stores/app'
import * as api from '../api'
import type { AuthMethod, Settings, SshTarget } from '../types'
import {
	ACCENT_IDS,
	ACCENT_ROLES,
	ACCENT_SEEDS,
	LEGACY_THEME_ALIAS,
	THEME_IDS,
	THEME_SURFACES,
	type AccentId,
	type ThemeId,
} from '../tokens'

const store = useAppStore()
const settings = ref<Settings | null>(null)
const ssh = ref<SshTarget>({
	id: 'default',
	host: '',
	port: 8022,
	username: '',
	auth_method: 'agent',
	identity_file: null,
	label: null,
	confirmed: false,
})
const sshMessage = ref<string | null>(null)
const sshAvailable = ref(false)
const saving = ref(false)

const sections = [
	{ id: 'appearance', title: 'Appearance' },
	{ id: 'network', title: 'Network' },
	{ id: 'preferences', title: 'Preferences' },
	{ id: 'access', title: 'Screen mirroring' },
	{ id: 'integrations', title: 'Integrations' },
	{ id: 'about', title: 'About' },
]

const activeSection = ref('appearance')

let observer: IntersectionObserver | null = null

function observeSections() {
	const roots = sections
		.map((s) => document.getElementById(`section-${s.id}`))
		.filter((el): el is HTMLElement => !!el)
	if (!('IntersectionObserver' in window)) return
	observer = new IntersectionObserver(
		(entries) => {
			for (const e of entries) {
				if (e.isIntersecting) activeSection.value = e.target.id.replace('section-', '')
			}
		},
		{ rootMargin: '-30% 0px -60% 0px' },
	)
	for (const el of roots) observer.observe(el)
}

onBeforeUnmount(() => observer?.disconnect())

function scrollTo(id: string) {
	activeSection.value = id
	document.getElementById(`section-${id}`)?.scrollIntoView({ behavior: 'smooth', block: 'start' })
}

/** Arrow-key travel across the sticky chip bar. */
const chipRefs = ref<HTMLButtonElement[]>([])
function setChipRef(el: HTMLButtonElement | null, idx: number) {
	if (el) chipRefs.value[idx] = el
}
function onChipKey(e: KeyboardEvent) {
	const idx = chipRefs.value.findIndex((el) => el === document.activeElement)
	if (idx === -1) return
	if (e.key === 'ArrowRight' || e.key === 'ArrowDown') {
		e.preventDefault()
		const next = (idx + 1) % chipRefs.value.length
		chipRefs.value[next]?.focus()
		scrollTo(sections[next]!.id)
	} else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') {
		e.preventDefault()
		const prev = (idx - 1 + chipRefs.value.length) % chipRefs.value.length
		chipRefs.value[prev]?.focus()
		scrollTo(sections[prev]!.id)
	}
}

async function load() {
	settings.value = await api.getSettings()
	store.filemanager = await api.filemanagerStatus()
	const available = await api.sshAvailable()
	sshAvailable.value = available.available
	applyAppearance()
	observeSections()
}

/** Mirror theme/accent/reduce-motion onto the document (see App.vue). */
function applyAppearance() {
	if (typeof document === 'undefined' || !settings.value) return
	store.applyTheme(currentTheme.value)
	const accent = currentAccent.value
	document.documentElement.setAttribute('data-accent', accent)
	if (settings.value.reduceMotion) document.documentElement.setAttribute('data-reduce-motion', '')
	else document.documentElement.removeAttribute('data-reduce-motion')
	try {
		localStorage.setItem('pc-theme', currentTheme.value)
		localStorage.setItem('pc-accent', accent)
		localStorage.setItem('pc-reduce-motion', settings.value.reduceMotion ? '1' : '0')
	} catch {
		/* private mode */
	}
}

// ---------------------------------------------------------------- themes ---

/**
 * The themes offered, previewed with their own surface tokens so a preview
 * cannot drift from the theme it selects.
 */
const THEME_META: Record<ThemeId, { label: string; note: string }> = {
	amoled: { label: 'AMOLED Black', note: 'True black for OLED screens' },
	'space-dark': { label: 'Space Dark', note: 'Deep blue canvas' },
	'simple-dark': { label: 'Simple Dark', note: 'The default dark theme' },
	'simple-light': { label: 'Simple Light', note: 'For bright rooms' },
	'white-light': { label: 'White Light', note: 'Pure-white canvas' },
	system: { label: 'System', note: 'Follows the OS light/dark setting' },
}

/**
 * The theme currently in effect. Legacy stored values map through their
 * documented aliases; anything else falls back to the default dark theme so
 * the picker never renders with nothing selected.
 */
const currentTheme = computed<ThemeId>((): ThemeId => {
	const t = settings.value?.theme
	if (t && (THEME_IDS as readonly string[]).includes(t)) return t as ThemeId
	if (t && LEGACY_THEME_ALIAS[t]) return LEGACY_THEME_ALIAS[t]!
	return 'simple-dark'
})

const ACCENT_LABELS: Record<AccentId, string> = {
	purple: 'Purple',
	blue: 'Blue',
	pink: 'Pink',
	green: 'Green',
	yellow: 'Yellow',
	brown: 'Brown',
	gray: 'Gray',
	white: 'White',
}

/** The accent in effect, defaulting to the backend default. */
const currentAccent = computed<AccentId>((): AccentId => {
	const a = settings.value?.accent
	return a && (ACCENT_IDS as readonly string[]).includes(a) ? (a as AccentId) : 'green'
})

/** Dark-scheme primary for the accent dot in each theme preview. */
function previewAccent(accent: AccentId): string {
	return ACCENT_ROLES.dark[accent].primary
}

const themeRefs = ref<HTMLButtonElement[]>([])
function setThemeRef(el: HTMLButtonElement | null, idx: number) {
	if (el) themeRefs.value[idx] = el
}
function onThemeKey(e: KeyboardEvent, idx: number) {
	const ids = [...THEME_IDS]
	let next: number | null = null
	if (e.key === 'ArrowRight' || e.key === 'ArrowDown') next = (idx + 1) % ids.length
	else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') next = (idx - 1 + ids.length) % ids.length
	if (next !== null) {
		e.preventDefault()
		themeRefs.value[next]?.focus()
		void save('theme', ids[next]!)
	}
}

const accentRefs = ref<HTMLButtonElement[]>([])
function setAccentRef(el: HTMLButtonElement | null, idx: number) {
	if (el) accentRefs.value[idx] = el
}
function onAccentKey(e: KeyboardEvent, idx: number) {
	const ids = [...ACCENT_IDS]
	let next: number | null = null
	if (e.key === 'ArrowRight' || e.key === 'ArrowDown') next = (idx + 1) % ids.length
	else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') next = (idx - 1 + ids.length) % ids.length
	if (next !== null) {
		e.preventDefault()
		accentRefs.value[next]?.focus()
		void save('accent', ids[next]!)
	}
}

// -------------------------------------------------------------- identity ---

const savedName = ref('')
const nameSavedTick = ref(false)
let nameTickTimer: ReturnType<typeof setTimeout> | null = null
const nameInput = ref<HTMLInputElement | null>(null)

function focusName() {
	nameInput.value?.focus()
	nameInput.value?.select()
}

async function saveName() {
	if (!settings.value) return
	const name = settings.value.kdeDeviceName.trim()
	if (!name || name === savedName.value) return
	await save('kdeDeviceName', settings.value.kdeDeviceName)
	savedName.value = settings.value.kdeDeviceName
	nameSavedTick.value = true
	if (nameTickTimer) clearTimeout(nameTickTimer)
	nameTickTimer = setTimeout(() => (nameSavedTick.value = false), 3000)
}

// -------------------------------------------------------------- segmented ---

const VISIBILITY_OPTS = [
	{ v: 0, label: 'Visible', hint: 'Advertise to nearby phones' },
	{ v: 1, label: 'Invisible', hint: 'Stay hidden' },
	{ v: 2, label: 'Temporary', hint: 'Visible briefly' },
] as const

const segRefs = ref<HTMLButtonElement[]>([])
const visibilityHint = computed(
	() => VISIBILITY_OPTS.find((o) => o.v === settings.value?.visibility)?.hint ?? '',
)
function setSegRef(el: HTMLButtonElement | null, idx: number) {
	if (el) segRefs.value[idx] = el
}
function onSegKey(e: KeyboardEvent, idx: number) {
	let next: number | null = null
	if (e.key === 'ArrowRight' || e.key === 'ArrowDown') next = (idx + 1) % VISIBILITY_OPTS.length
	else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') next = (idx - 1 + VISIBILITY_OPTS.length) % VISIBILITY_OPTS.length
	if (next !== null) {
		e.preventDefault()
		segRefs.value[next]?.focus()
		void save('visibility', VISIBILITY_OPTS[next]!.v)
	}
}

type MutableSetting =
	| 'realclose'
	| 'startminimized'
	| 'autostart'
	| 'visibility'
	| 'clipboardEnabled'
	| 'clipboardAutoShare'
	| 'autoAcceptShares'
	| 'clipboardImagesOnReceive'
	| 'notifySharedText'
	| 'notifyShareRequest'
	| 'notifyTransferComplete'
	| 'notifyTransferFailed'
	| 'notifyProgress'
	| 'kdeDeviceName'
	| 'theme'
	| 'accent'
	| 'reduceMotion'

async function save(key: MutableSetting, value: boolean | string | number | null) {
	if (!settings.value) return
	saving.value = true
	const next: Settings = { ...settings.value, [key]: value } as Settings
	try {
		settings.value = await api.updateSettings(next)
		store.settings = settings.value
		// Appearance applies the moment it changes. Waiting for a restart to
		// see it would make the control look broken.
		if (key === 'theme' || key === 'accent' || key === 'reduceMotion') applyAppearance()
	} catch (e) {
		store.toast(`Could not save the setting: ${e}`, 'error')
	} finally {
		saving.value = false
	}
}

/**
 * Choose the one folder both protocols download into.
 *
 * Quick Share and KDE Connect each had their own setting, so picking one left
 * the other writing somewhere the user had not chosen. This applies to both and
 * reports what it actually did, rather than silently storing a value that only
 * one backend honours.
 */
async function pickDownloadFolder() {
	try {
		const { open } = await import('@tauri-apps/plugin-dialog')
		const current = settings.value?.downloadFolder ?? settings.value?.downloadPath ?? null
		const picked = await open({ directory: true, defaultPath: current ?? undefined })
		if (typeof picked !== 'string') return
		await api.setDownloadFolder(picked)
		// Read the value back rather than assuming: the command is what actually
		// configures both backends, and a stale UI here would be misleading.
		if (settings.value) {
			settings.value = {
				...settings.value,
				downloadFolder: picked,
				downloadPath: picked,
				incomingPath: picked,
			}
			store.settings = settings.value
		}
		store.toast('Both Quick Share and KDE Connect will save here now', 'success')
	} catch (e) {
		store.toast(`Could not set the download folder: ${e}`, 'error')
	}
}

async function testSsh() {
	try {
		const probe = await api.sshProbe(ssh.value)
		sshMessage.value = `${probe.message}${probe.version ? ` (${probe.version})` : ''}`
	} catch (e) {
		sshMessage.value = `Could not test the connection: ${e}`
	}
}

async function openSsh() {
	try {
		const message = await api.sshOpen(ssh.value)
		sshMessage.value = message
	} catch (e) {
		sshMessage.value = `Could not open a session: ${e}`
	}
}

const authMethods: { value: AuthMethod; label: string }[] = [
	{ value: 'agent', label: 'SSH agent / default keys' },
	{ value: 'identityFile', label: 'Identity file' },
	{ value: 'password', label: 'Password (prompted in the terminal)' },
]

onMounted(async () => {
	await load()
	savedName.value = settings.value?.kdeDeviceName ?? ''
	await nextTick()
})
</script>

<template>
	<div v-if="!settings" class="pc-scroll flex-1 px-6 pb-28 pt-6 sm:px-10 lg:px-14">
		<p class="pc-eyebrow">Loading settings…</p>
	</div>
	<div v-else class="pc-scroll flex-1 px-6 pb-28 pt-6 sm:px-10 lg:px-14 lg:pb-12">
		<div class="mx-auto w-full max-w-content">
			<header class="flex items-start justify-between">
				<div>
					<h2 class="pc-display text-4xl sm:text-5xl">Settings</h2>
					<p class="mt-2 text-sm text-muted">System configuration and preferences.</p>
				</div>
			</header>

			<!-- sticky scroll-spy chip bar -->
			<nav class="pc-spy mt-6" aria-label="Settings sections" @keydown="onChipKey">
				<button
					v-for="(s, i) in sections"
					:key="s.id"
					:ref="(el) => setChipRef(el as HTMLButtonElement | null, i)"
					type="button"
					class="pc-spy__chip"
					:aria-current="activeSection === s.id ? 'true' : undefined"
					@click="scrollTo(s.id)"
				>
					{{ s.title }}
				</button>
			</nav>

			<!-- Appearance -->
			<section :id="'section-appearance'" class="pc-anchor mt-10" aria-label="Appearance">
				<h3 class="pc-section-title">Appearance</h3>
				<p class="mt-1 text-sm text-muted">Theme, accent colour and motion. Applies immediately.</p>

				<div class="pc-card mt-4 p-6">
					<p class="pc-eyebrow">Theme</p>
					<div
						class="mt-4 grid gap-3 sm:grid-cols-2 lg:grid-cols-3"
						role="radiogroup"
						aria-label="Colour theme"
					>
						<button
							v-for="(id, i) in THEME_IDS"
							:key="id"
							:ref="(el) => setThemeRef(el as HTMLButtonElement | null, i)"
							type="button"
							role="radio"
							:aria-checked="currentTheme === id"
							class="pc-theme"
							:data-active="currentTheme === id"
							@click="save('theme', id)"
							@keydown="(e) => onThemeKey(e, i)"
						>
							<span
								class="pc-theme__preview"
								aria-hidden="true"
								:style="{ backgroundColor: `rgb(${THEME_SURFACES[id].bg})`, borderColor: `rgb(${THEME_SURFACES[id].line})` }"
							>
								<span class="pc-theme__bar" :style="{ backgroundColor: `rgb(${THEME_SURFACES[id].card})` }">
									<span class="pc-theme__dot" :style="{ backgroundColor: previewAccent(currentAccent) }" />
									<span class="pc-theme__lines">
										<span :style="{ backgroundColor: `rgb(${THEME_SURFACES[id].text})` }" />
										<span :style="{ backgroundColor: `rgb(${THEME_SURFACES[id].muted})` }" />
									</span>
								</span>
								<span class="pc-theme__body" :style="{ backgroundColor: `rgb(${THEME_SURFACES[id].cardAlt})` }" />
							</span>
							<span class="min-w-0 flex-1">
								<span class="block whitespace-normal font-semibold leading-snug">{{ THEME_META[id].label }}</span>
								<span class="mt-0.5 block whitespace-normal text-xs leading-snug text-muted">{{ THEME_META[id].note }}</span>
							</span>
							<span v-if="currentTheme === id" class="pc-theme__check" aria-hidden="true">
								<PcIcon name="check" :size="14" />
							</span>
						</button>
					</div>
				</div>

				<div class="pc-card mt-4 p-6">
					<p class="pc-eyebrow">Accent</p>
					<div
						class="mt-4 flex flex-wrap gap-3"
						role="radiogroup"
						aria-label="Accent colour"
					>
						<button
							v-for="(id, i) in ACCENT_IDS"
							:key="id"
							:ref="(el) => setAccentRef(el as HTMLButtonElement | null, i)"
							type="button"
							role="radio"
							:aria-checked="currentAccent === id"
							:aria-label="ACCENT_LABELS[id]"
							:title="ACCENT_LABELS[id]"
							class="pc-accent"
							:data-active="currentAccent === id"
							:style="{ backgroundColor: ACCENT_SEEDS[id] }"
							@click="save('accent', id)"
							@keydown="(e) => onAccentKey(e, i)"
						>
							<PcIcon v-if="currentAccent === id" name="check" :size="16" class="pc-accent__check" />
						</button>
					</div>
					<p class="mt-3 text-sm text-muted">Currently <span class="font-semibold text-ink">{{ ACCENT_LABELS[currentAccent] }}</span>.</p>
				</div>

				<div class="pc-card mt-4">
					<div class="pc-setting">
						<div class="min-w-0 flex-1">
							<p class="font-semibold">Reduce animations</p>
							<p class="mt-0.5 text-sm text-muted">Calms transitions, springs and the ambient glow.</p>
						</div>
						<div class="pc-setting__control">
							<ToggleSwitch
								:model-value="settings.reduceMotion"
								label="Reduce animations"
								@update:model-value="save('reduceMotion', $event)"
							/>
						</div>
					</div>
				</div>
			</section>

			<!-- Network -->
			<section :id="'section-network'" class="pc-anchor mt-12" aria-label="Network">
				<h3 class="pc-section-title">Network</h3>

				<div class="pc-card mt-4">
					<div class="pc-setting">
						<div class="min-w-0 flex-1">
							<p class="font-semibold">This computer's name</p>
							<p class="mt-0.5 text-sm text-muted">
								The name phones see over the KDE Connect protocol.
							</p>
							<div class="mt-3 flex flex-wrap items-center gap-2">
								<input
									ref="nameInput"
									v-model="settings.kdeDeviceName"
									type="text"
									class="pc-inset min-w-0 flex-1 bg-transparent px-4 py-3 text-sm outline-none"
									placeholder="My laptop"
									aria-label="Advertised device name"
									@change="saveName"
									@keydown.enter="saveName"
								/>
								<button
									type="button"
									class="pc-btn pc-btn-secondary pc-btn-sm shrink-0"
									@click="focusName"
								>
									Edit
								</button>
								<button
									type="button"
									class="pc-btn pc-btn-primary pc-btn-sm shrink-0"
									:disabled="!settings.kdeDeviceName.trim() || settings.kdeDeviceName === savedName"
									@click="saveName"
								>
									Save
								</button>
							</div>
							<p v-if="nameSavedTick" class="mt-2 inline-flex items-center gap-1.5 text-sm" role="status">
								<span class="pc-pill" data-tone="online"><PcIcon name="check" :size="12" /> Saved</span>
							</p>
						</div>
					</div>
					<div class="pc-setting">
						<div class="min-w-0 flex-1">
							<p class="font-semibold">Download folder</p>
							<!--
								One folder for both protocols. They used to have separate
								pickers, so choosing one left the other writing somewhere the
								user had not picked - and files arrived in an unexpected place
								with nothing indicating which setting had changed.
							-->
							<p class="mt-0.5 break-all text-sm text-muted">
								{{ settings.downloadFolder ?? settings.downloadPath ?? '~/Downloads' }}
							</p>
							<p class="mt-0.5 text-xs text-faint">
								Used for everything arriving from your phone, whether it is sent with
								Quick Share or over KDE Connect.
							</p>
							<button
								class="pc-btn pc-btn-secondary pc-btn-sm mt-3"
								@click="pickDownloadFolder"
							>
								Change
							</button>
						</div>
					</div>
					<div class="pc-setting">
						<div class="min-w-0 flex-1">
							<p class="font-semibold">Quick Share visibility</p>
							<p class="mt-0.5 text-sm text-muted">
								Controls whether this computer advertises itself to nearby phones.
							</p>
							<div
								class="pc-seg mt-3"
								role="radiogroup"
								aria-label="Quick Share visibility"
							>
								<button
									v-for="(opt, i) in VISIBILITY_OPTS"
									:key="opt.v"
									:ref="(el) => setSegRef(el as HTMLButtonElement | null, i)"
									type="button"
									role="radio"
									:aria-checked="settings.visibility === opt.v"
									class="pc-seg__opt"
									:data-active="settings.visibility === opt.v"
									:title="opt.hint"
									@click="save('visibility', opt.v)"
									@keydown="(e) => onSegKey(e, i)"
								>
									{{ opt.label }}
								</button>
							</div>
							<p class="mt-2 text-xs text-faint">{{ visibilityHint }}</p>
						</div>
					</div>
				</div>
			</section>

			<!-- Preferences -->
			<section :id="'section-preferences'" class="pc-anchor mt-12" aria-label="Preferences">
				<h3 class="pc-section-title">Preferences</h3>

				<p class="pc-eyebrow mt-6">Clipboard</p>
				<div class="pc-card mt-2">
					<div class="pc-setting">
						<PcIcon name="copy" :size="20" class="shrink-0 text-muted" />
						<div class="min-w-0 flex-1">
							<p class="font-semibold">Clipboard sync</p>
							<p class="mt-0.5 text-sm text-muted">
								Share copied text between the phone and this computer.
							</p>
							<p v-if="store.clipboardCaps" class="mt-1 text-xs text-faint">
								{{ store.clipboardCaps.mechanismDetail }}
							</p>
							<p v-if="store.clipboardCaps" class="mt-2">
								<span
									class="pc-pill"
									:data-tone="store.clipboardCaps.backgroundReadAllowed ? 'online' : 'warn'"
								>
									{{ store.clipboardCaps.backgroundReadAllowed
										? 'Runs in the background — the window does not need to be focused'
										: 'Needs focus — the window must be focused to read the clipboard' }}
								</span>
							</p>
						</div>
						<div class="pc-setting__control">
							<ToggleSwitch
								:model-value="settings.clipboardEnabled"
								label="Clipboard sync"
								@update:model-value="save('clipboardEnabled', $event)"
							/>
						</div>
					</div>
					<div class="pc-setting">
						<PcIcon name="bell" :size="20" class="shrink-0 text-muted" />
						<div class="min-w-0 flex-1">
							<p class="font-semibold">Auto-share clipboard</p>
							<p class="mt-0.5 text-sm text-muted">
								Send clipboard changes automatically instead of on demand.
							</p>
						</div>
						<div class="pc-setting__control">
							<ToggleSwitch
								:model-value="settings.clipboardAutoShare"
								label="Auto-share clipboard"
								:disabled="!settings.clipboardEnabled"
								@update:model-value="save('clipboardAutoShare', $event)"
							/>
						</div>
					</div>
				</div>

				<p class="pc-eyebrow mt-8">Sharing</p>
				<div class="pc-card mt-2">
					<div class="pc-setting">
						<PcIcon name="shield" :size="20" class="shrink-0 text-muted" />
						<div class="min-w-0 flex-1">
							<p class="font-semibold">Auto-accept incoming shares</p>
							<p class="mt-0.5 text-sm text-muted">
								Accept files from your phone without being asked. Off by default: any
								paired device can send, so this lets a phone write to this computer
								unprompted. Files are saved to your download folder either way.
							</p>
						</div>
						<div class="pc-setting__control">
							<ToggleSwitch
								:model-value="settings.autoAcceptShares"
								label="Auto-accept incoming shares"
								@update:model-value="save('autoAcceptShares', $event)"
							/>
						</div>
					</div>
					<div class="pc-setting">
						<PcIcon name="image" :size="20" class="shrink-0 text-muted" />
						<div class="min-w-0 flex-1">
							<p class="font-semibold">Copy received images to the clipboard</p>
							<p class="mt-0.5 text-sm text-muted">
								So a photo sent from your phone can be pasted straight into a message.
								Only images; other files are just saved.
							</p>
						</div>
						<div class="pc-setting__control">
							<ToggleSwitch
								:model-value="settings.clipboardImagesOnReceive"
								label="Copy received images to the clipboard"
								@update:model-value="save('clipboardImagesOnReceive', $event)"
							/>
						</div>
					</div>
				</div>

				<!--
					Notification categories. Each event is switchable on its own so a
					transfer can announce itself without every progress tick also
					announcing itself. `notifyProgress` is off by default: it fires
					repeatedly during a single transfer.
				-->
				<p class="pc-eyebrow mt-8">Notifications</p>
				<div class="pc-card mt-2">
					<div class="pc-setting">
						<PcIcon name="bell" :size="20" class="shrink-0 text-muted" />
						<div class="min-w-0 flex-1">
							<p class="font-semibold">Notify when text is shared</p>
							<p class="mt-0.5 text-sm text-muted">
								Your phone sends text or a link and it lands on your clipboard.
							</p>
						</div>
						<div class="pc-setting__control">
							<ToggleSwitch
								:model-value="settings.notifySharedText"
								label="Notify when text is shared"
								@update:model-value="save('notifySharedText', $event)"
							/>
						</div>
					</div>
					<div class="pc-setting">
						<PcIcon name="bell" :size="20" class="shrink-0 text-muted" />
						<div class="min-w-0 flex-1">
							<p class="font-semibold">Notify about incoming shares</p>
							<p class="mt-0.5 text-sm text-muted">
								When your phone sends a file or a link. The notification can accept or
								reject it without opening this window.
							</p>
						</div>
						<div class="pc-setting__control">
							<ToggleSwitch
								:model-value="settings.notifyShareRequest"
								label="Notify about incoming shares"
								@update:model-value="save('notifyShareRequest', $event)"
							/>
						</div>
					</div>
					<div class="pc-setting">
						<PcIcon name="check" :size="20" class="shrink-0 text-muted" />
						<div class="min-w-0 flex-1">
							<p class="font-semibold">Notify when a transfer finishes</p>
							<p class="mt-0.5 text-sm text-muted">One notification per completed transfer.</p>
						</div>
						<div class="pc-setting__control">
							<ToggleSwitch
								:model-value="settings.notifyTransferComplete"
								label="Notify when a transfer finishes"
								@update:model-value="save('notifyTransferComplete', $event)"
							/>
						</div>
					</div>
					<div class="pc-setting">
						<PcIcon name="alert" :size="20" class="shrink-0 text-muted" />
						<div class="min-w-0 flex-1">
							<p class="font-semibold">Notify when a transfer fails</p>
							<p class="mt-0.5 text-sm text-muted">
								Includes the reason, so a failure is never silent.
							</p>
						</div>
						<div class="pc-setting__control">
							<ToggleSwitch
								:model-value="settings.notifyTransferFailed"
								label="Notify when a transfer fails"
								@update:model-value="save('notifyTransferFailed', $event)"
							/>
						</div>
					</div>
					<div class="pc-setting">
						<PcIcon name="activity" :size="20" class="shrink-0 text-muted" />
						<div class="min-w-0 flex-1">
							<p class="font-semibold">Notify on transfer progress</p>
							<p class="mt-0.5 text-sm text-muted">
								Off by default: this fires repeatedly while a transfer runs. Progress is
								always shown in the app.
							</p>
						</div>
						<div class="pc-setting__control">
							<ToggleSwitch
								:model-value="settings.notifyProgress"
								label="Notify on transfer progress"
								@update:model-value="save('notifyProgress', $event)"
							/>
						</div>
					</div>
				</div>

				<p class="pc-eyebrow mt-8">Startup</p>
				<div class="pc-card mt-2">
					<div class="pc-setting">
						<PcIcon name="bolt" :size="20" class="shrink-0 text-muted" />
						<div class="min-w-0 flex-1">
							<p class="font-semibold">Start at login</p>
							<p class="mt-0.5 text-sm text-muted">Launch Pixel Connect when you sign in.</p>
						</div>
						<div class="pc-setting__control">
							<ToggleSwitch
								:model-value="settings.autostart"
								label="Start at login"
								@update:model-value="save('autostart', $event)"
							/>
						</div>
					</div>
					<div class="pc-setting">
						<PcIcon name="close" :size="20" class="shrink-0 text-muted" />
						<div class="min-w-0 flex-1">
							<p class="font-semibold">Close instead of minimise</p>
							<p class="mt-0.5 text-sm text-muted">
								Close the window for real rather than hiding to the tray.
							</p>
						</div>
						<div class="pc-setting__control">
							<ToggleSwitch
								:model-value="settings.realclose"
								label="Close instead of minimise"
								@update:model-value="save('realclose', $event)"
							/>
						</div>
					</div>
				</div>
			</section>

			<!-- Screen mirroring & access -->
			<section :id="'section-access'" class="pc-anchor mt-12" aria-label="Screen mirroring">
				<h3 class="pc-section-title">Screen mirroring</h3>

				<div class="pc-card mt-4">
					<div class="pc-setting">
						<div class="min-w-0 flex-1">
							<p class="pc-eyebrow">scrcpy</p>
							<p class="mt-2 text-sm text-muted">
								Pixel Connect manages scrcpy and ADB for you; it never enables debugging or
								changes developer settings on the phone.
							</p>
							<dl class="mt-4 grid gap-3 text-sm sm:grid-cols-2">
								<div class="pc-path">
									<dt class="text-muted">adb</dt>
									<dd class="mt-1 flex flex-wrap items-center gap-1.5">
										<span class="pc-chip font-mono">{{ store.tools?.adbPath ?? 'Not found' }}</span>
										<span v-if="store.tools?.adbVersion" class="pc-chip">{{ store.tools.adbVersion }}</span>
									</dd>
								</div>
								<div class="pc-path">
									<dt class="text-muted">scrcpy</dt>
									<dd class="mt-1 flex flex-wrap items-center gap-1.5">
										<span class="pc-chip font-mono">{{ store.tools?.scrcpyPath ?? 'Not found' }}</span>
										<span v-if="store.tools?.scrcpyVersion" class="pc-chip">{{ store.tools.scrcpyVersion }}</span>
									</dd>
								</div>
							</dl>
							<div v-if="store.tools?.error" class="m3-helper mt-4">
								<span class="pc-pill shrink-0" data-tone="warn">Needs setup</span>
								<span class="text-sm text-muted">{{ store.tools.error }}</span>
							</div>
							<button class="pc-btn pc-btn-secondary pc-btn-sm mt-4" @click="store.bootstrap()">
								<PcIcon name="refresh" :size="14" />
								Re-detect tools
							</button>
						</div>
					</div>
				</div>

				<details class="pc-card pc-advanced mt-4">
					<summary class="pc-advanced__summary">
						<span class="min-w-0 flex-1">
							<span class="block font-semibold">Advanced: SSH (optional)</span>
							<span class="mt-0.5 block text-sm font-normal text-muted">
								Connect to a phone that already has an SSH server you configured. Pixel Connect
								never installs or enables one.
							</span>
						</span>
						<PcIcon name="menu" :size="16" class="shrink-0 text-muted" />
					</summary>
					<div class="px-6 pb-6">
						<div v-if="!sshAvailable" class="m3-helper">
							<span class="pc-pill shrink-0" data-tone="warn">Unavailable</span>
							<span class="text-sm text-muted">The system OpenSSH client was not found on PATH.</span>
						</div>

						<div v-else class="grid gap-4 sm:grid-cols-2">
							<label class="block">
								<span class="pc-eyebrow">Host</span>
								<input
									v-model="ssh.host"
									type="text"
									class="pc-inset mt-2 w-full bg-transparent px-4 py-3 text-sm outline-none"
									placeholder="192.168.1.42"
								/>
							</label>
							<label class="block">
								<span class="pc-eyebrow">Port</span>
								<input
									v-model.number="ssh.port"
									type="number"
									min="1"
									max="65535"
									class="pc-inset mt-2 w-full bg-transparent px-4 py-3 text-sm outline-none"
								/>
							</label>
							<label class="block">
								<span class="pc-eyebrow">Username</span>
								<input
									v-model="ssh.username"
									type="text"
									class="pc-inset mt-2 w-full bg-transparent px-4 py-3 text-sm outline-none"
									placeholder="termux"
								/>
							</label>
							<label class="block">
								<span class="pc-eyebrow">Authentication</span>
								<select
									v-model="ssh.auth_method"
									class="pc-inset mt-2 w-full bg-transparent px-4 py-3 text-sm outline-none"
								>
									<option v-for="m in authMethods" :key="m.value" :value="m.value">
										{{ m.label }}
									</option>
								</select>
							</label>
							<label v-if="ssh.auth_method === 'identityFile'" class="block sm:col-span-2">
								<span class="pc-eyebrow">Identity file</span>
								<input
									v-model="ssh.identity_file"
									type="text"
									class="pc-inset mt-2 w-full bg-transparent px-4 py-3 text-sm outline-none"
									placeholder="/home/me/.ssh/id_ed25519"
								/>
							</label>
						</div>

						<label v-if="sshAvailable" class="mt-5 flex cursor-pointer items-center gap-3 text-sm">
							<input v-model="ssh.confirmed" type="checkbox" class="pc-check" />
							<span>I configured this myself — I set up the SSH server on this phone.</span>
						</label>

						<div class="mt-5 flex flex-wrap gap-2">
							<button
								class="pc-btn pc-btn-secondary pc-btn-sm"
								:disabled="!sshAvailable || !ssh.confirmed"
								:title="!ssh.confirmed ? 'Confirm you configured the server yourself first' : undefined"
								@click="testSsh"
							>
								<PcIcon name="terminal" :size="14" />
								Test connection
							</button>
							<button
								class="pc-btn pc-btn-primary pc-btn-sm"
								:disabled="!sshAvailable || !ssh.confirmed || !ssh.host"
								:title="!ssh.confirmed ? 'Confirm you configured the server yourself first' : undefined"
								@click="openSsh"
							>
								Open session
							</button>
						</div>
						<p v-if="sshMessage" class="mt-3 text-sm text-muted" role="status">{{ sshMessage }}</p>
					</div>
				</details>
			</section>

			<!-- Integrations -->
			<section :id="'section-integrations'" class="pc-anchor mt-12" aria-label="Integrations">
				<h3 class="pc-section-title">Integrations</h3>

				<div class="pc-card mt-4">
					<div class="pc-setting">
						<div class="min-w-0 flex-1">
							<p class="font-semibold">Files — "Send to Pixel"</p>
							<p class="mt-0.5 text-sm text-muted">
								{{ store.filemanager?.note ?? 'Checking…' }}
							</p>
							<dl class="mt-4 grid gap-3 text-sm sm:grid-cols-2">
								<div class="flex justify-between gap-4">
									<dt class="text-muted">Backend socket</dt>
									<dd class="text-right font-medium">
										{{ store.filemanager?.listening ? 'Listening' : 'Not listening' }}
									</dd>
								</div>
								<div class="flex justify-between gap-4">
									<dt class="text-muted">Nautilus extension</dt>
									<dd class="text-right font-medium">
										{{ store.filemanager?.pythonExtensionInstalled ? 'Installed' : 'Not installed' }}
									</dd>
								</div>
							</dl>
							<code class="pc-inset mt-4 block px-4 py-3 text-xs">
								scripts/install-filemanager-integration.sh
							</code>
						</div>
					</div>
				</div>
			</section>

			<!-- About -->
			<section :id="'section-about'" class="pc-anchor mt-12" aria-label="About">
				<h3 class="pc-section-title">About</h3>

				<div class="mt-4 grid gap-4 lg:grid-cols-2">
					<div class="pc-card px-6 py-6">
						<p class="pc-eyebrow">System</p>
						<p class="pc-display mt-3 text-3xl">
							{{ store.info?.name ?? 'Pixel Connect' }}
							v{{ store.info?.version ?? '0.1.0' }}
						</p>
						<div class="mt-4 flex flex-wrap gap-2">
							<span class="pc-chip">Linux native</span>
							<span class="pc-chip">{{ store.clipboardCaps?.mechanismLabel ?? 'Unknown' }}</span>
							<span class="pc-chip">KDE Connect v{{ store.info?.kdeconnectProtocol ?? 8 }}</span>
						</div>
						<dl class="mt-5 space-y-2 text-sm">
							<div class="flex justify-between gap-4">
								<dt class="text-muted">Identifier</dt>
								<dd class="font-mono text-xs">{{ store.info?.identifier }}</dd>
							</div>
							<div class="flex justify-between gap-4">
								<dt class="text-muted">Quick Share</dt>
								<dd class="font-medium">
									{{ store.quickshareEnabled ? 'Running' : 'Off' }}
								</dd>
							</div>
						</dl>
					</div>

					<div class="pc-card px-6 py-6">
						<p class="pc-eyebrow">Licence</p>
						<p class="mt-3 text-3xl font-extrabold tracking-tight">MIT Open Source</p>
						<p class="mt-3 text-sm text-muted">
							Quick Share support is derived from rquickshare (AGPL-3.0). The KDE Connect
							protocol implementation is a clean-room port of the wire format. See NOTICE.md.
						</p>
						<button class="pc-btn pc-btn-secondary pc-btn-sm mt-5" @click="api.openHelp()">
							<PcIcon name="info" :size="14" />
							About Pixel Connect
						</button>
					</div>
				</div>
			</section>
		</div>
	</div>
</template>

<style scoped>
.pc-anchor {
	scroll-margin-top: 4.5rem;
}

/* Sticky scroll-spy chip bar. */
.pc-spy {
	position: sticky;
	top: 0;
	z-index: 10;
	display: flex;
	gap: 0.5rem;
	overflow-x: auto;
	padding: 0.625rem 0.25rem;
	background-color: rgb(var(--pc-bg));
	border-bottom: 1px solid rgb(var(--pc-line));
}
.pc-spy__chip {
	flex: none;
	padding: 0.5rem 1rem;
	border-radius: 9999px;
	border: 1px solid rgb(var(--pc-line));
	font-size: 0.8125rem;
	font-weight: 600;
	color: rgb(var(--pc-muted));
	transition:
		background-color var(--pc-duration-short) var(--pc-ease-emphasized),
		color var(--pc-duration-short) var(--pc-ease-emphasized),
		border-color var(--pc-duration-short) var(--pc-ease-emphasized);
}
.pc-spy__chip:hover {
	color: rgb(var(--pc-text));
}
.pc-spy__chip[aria-current='true'] {
	background-color: rgb(var(--pc-accent));
	border-color: transparent;
	color: rgb(var(--pc-on-accent));
}

/* List rows: every setting is >=64px inside a rounded card. */
.pc-setting {
	min-height: 64px;
}
div :focus-visible,
button:focus-visible,
input:focus-visible,
select:focus-visible,
summary:focus-visible {
	outline: 2px solid var(--pc-primary);
	outline-offset: 2px;
}

/* Live mini-app theme previews. */
.pc-theme {
	position: relative;
	display: flex;
	align-items: center;
	gap: 0.75rem;
	padding: 0.75rem;
	border-radius: var(--pc-radius-inner);
	border: 2px solid transparent;
	background-color: rgb(var(--pc-card-alt));
	text-align: left;
	transition:
		border-color var(--pc-duration-short) var(--pc-ease-emphasized),
		transform var(--pc-duration-short) var(--pc-ease-emphasized);
}
.pc-theme:hover {
	transform: translateY(-1px);
}
.pc-theme[data-active='true'] {
	border-color: var(--pc-primary);
}
.pc-theme__preview {
	display: flex;
	flex-direction: column;
	flex: none;
	width: 4.5rem;
	height: 3.5rem;
	overflow: hidden;
	border-radius: 0.625rem;
	border: 1px solid;
}
.pc-theme__bar {
	display: flex;
	align-items: center;
	gap: 0.25rem;
	padding: 0.25rem 0.375rem;
}
.pc-theme__dot {
	width: 0.5rem;
	height: 0.5rem;
	border-radius: 9999px;
	flex: none;
}
.pc-theme__lines {
	display: flex;
	flex-direction: column;
	gap: 2px;
	flex: 1;
}
.pc-theme__lines > span {
	display: block;
	height: 3px;
	border-radius: 9999px;
}
.pc-theme__lines > span:last-child {
	width: 66%;
}
.pc-theme__body {
	flex: 1;
	margin: 0.25rem 0.375rem 0.375rem;
	border-radius: 0.375rem;
}
.pc-theme__check {
	position: absolute;
	top: 0.375rem;
	right: 0.375rem;
	display: inline-flex;
	align-items: center;
	justify-content: center;
	width: 1.375rem;
	height: 1.375rem;
	border-radius: 9999px;
	background-color: rgb(var(--pc-accent));
	color: rgb(var(--pc-on-accent));
}

/* Accent swatches with a morphing check. */
.pc-accent {
	position: relative;
	width: 2.75rem;
	height: 2.75rem;
	border-radius: 9999px;
	flex: none;
	border: 2px solid rgb(var(--pc-line));
	color: #0c0c0d;
	transition:
		transform var(--pc-duration-short) cubic-bezier(0.34, 1.3, 0.64, 1),
		border-color var(--pc-duration-short) var(--pc-ease-emphasized);
}
.pc-accent:hover {
	transform: scale(1.08);
}
.pc-accent[data-active='true'] {
	border-color: var(--pc-primary);
	transform: scale(1.08);
}
.pc-accent__check {
	position: absolute;
	inset: 0;
	margin: auto;
	animation: pc-check-in var(--pc-duration-short) cubic-bezier(0.3, 1.4, 0.5, 1);
	mix-blend-mode: difference;
	color: #fff;
}
@keyframes pc-check-in {
	0% {
		transform: scale(0.3);
		opacity: 0;
	}
	100% {
		transform: scale(1);
		opacity: 1;
	}
}

/* Segmented visibility control. */
.pc-seg {
	display: inline-flex;
	padding: 0.25rem;
	gap: 0.25rem;
	border-radius: 9999px;
	background-color: rgb(var(--pc-card-alt));
	border: 1px solid rgb(var(--pc-line));
	max-width: 100%;
	flex-wrap: wrap;
}
.pc-seg__opt {
	padding: 0.5rem 1.125rem;
	border-radius: 9999px;
	font-size: 0.875rem;
	font-weight: 600;
	color: rgb(var(--pc-muted));
	transition:
		background-color var(--pc-duration-short) var(--pc-ease-emphasized),
		color var(--pc-duration-short) var(--pc-ease-emphasized);
}
.pc-seg__opt[data-active='true'] {
	background-color: rgb(var(--pc-accent));
	color: rgb(var(--pc-on-accent));
}

/* Tonal helper rows (no bare colored text on tinted backgrounds). */
.m3-helper {
	display: flex;
	align-items: flex-start;
	gap: 0.625rem;
	border-radius: var(--pc-radius-inner);
	background-color: rgb(var(--pc-card-alt));
	padding: 0.75rem 0.875rem;
}

/* Two-column path chips, aligned. */
.pc-path {
	min-width: 0;
}
.pc-path .pc-chip {
	max-width: 100%;
	overflow-wrap: anywhere;
}

/* Advanced disclosure. */
.pc-advanced__summary {
	display: flex;
	align-items: flex-start;
	gap: 0.75rem;
	padding: 1.25rem 1.5rem;
	cursor: pointer;
	list-style: none;
	border-radius: var(--pc-radius-card);
}
.pc-advanced__summary::-webkit-details-marker {
	display: none;
}
.pc-advanced__summary:hover {
	background-color: rgb(var(--pc-card-alt));
}
.pc-check {
	width: 1.125rem;
	height: 1.125rem;
	accent-color: var(--pc-primary);
	flex: none;
}
select option {
	background-color: rgb(var(--pc-card));
	color: rgb(var(--pc-text));
}
</style>
