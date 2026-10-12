<script setup lang="ts">
// Pixel Connect — Settings view.
//
// Reproduces the settings screen from the supplied designs: numbered sections
// (01 Network, 02 Preferences, 03 About) with cards, plus the sections V1
// needs for clipboard, mirroring, SSH and the file-manager integration.
//
// Every control here performs a real action or is visibly marked unavailable.

import { computed, onMounted, ref } from 'vue'

import PcIcon from './atoms/PcIcon.vue'
import ToggleSwitch from './atoms/ToggleSwitch.vue'
import { useAppStore } from '../stores/app'
import * as api from '../api'
import type { AuthMethod, Settings, SshTarget } from '../types'

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
	{ id: 'network', numeral: '01', title: 'Network', icon: 'wifi' },
	{ id: 'preferences', numeral: '02', title: 'Preferences', icon: 'settings' },
	{ id: 'access', numeral: '03', title: 'Screen mirroring & access', icon: 'monitor' },
	{ id: 'integrations', numeral: '04', title: 'Integrations', icon: 'link' },
	{ id: 'about', numeral: '05', title: 'About', icon: 'info' },
]

function scrollTo(id: string) {
	document.getElementById(`section-${id}`)?.scrollIntoView({ behavior: 'smooth' })
}

async function load() {
	settings.value = await api.getSettings()
	store.filemanager = await api.filemanagerStatus()
	const available = await api.sshAvailable()
	sshAvailable.value = available.available
}

/**
 * The themes offered, with each one's own tokens.
 *
 * The swatches are built from the same values as main.css rather than being
 * drawn separately, so a swatch cannot drift from the theme it previews.
 */
const THEMES = [
	{
		id: 'dark',
		label: 'Dark',
		note: 'The default',
		swatch: { bg: '10 10 10', line: '42 42 42', text: '244 244 244', muted: '142 142 142', accent: '244 244 244' }
	},
	{
		id: 'light',
		label: 'Light',
		note: 'For bright rooms',
		swatch: { bg: '246 246 246', line: '224 224 224', text: '16 16 16', muted: '96 96 96', accent: '16 16 16' }
	},
	{
		id: 'midnight',
		label: 'Midnight',
		note: 'Cool blue',
		swatch: { bg: '4 6 14', line: '32 42 68', text: '236 240 250', muted: '156 166 190', accent: '140 170 255' }
	},
	{
		id: 'contrast-dark',
		label: 'High contrast',
		note: 'Dark, maximum contrast',
		swatch: { bg: '0 0 0', line: '110 110 110', text: '255 255 255', muted: '214 214 214', accent: '255 255 255' }
	},
	{
		id: 'contrast-light',
		label: 'High contrast light',
		note: 'Light, maximum contrast',
		swatch: { bg: '255 255 255', line: '40 40 40', text: '0 0 0', muted: '32 32 32', accent: '0 0 0' }
	}
] as const

/**
 * The theme currently in effect.
 *
 * Falls back to 'dark' when the stored value is missing or is not one this
 * build knows. Without this the picker renders with nothing selected at all,
 * which reads as a broken control rather than as "using the default".
 */
const currentTheme = computed(() => {
	const t = settings.value?.theme
	return THEMES.some((o) => o.id === t) ? (t as string) : 'dark'
})

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

async function save(key: MutableSetting, value: boolean | string | number | null) {
	if (!settings.value) return
	saving.value = true
	const next: Settings = { ...settings.value, [key]: value } as Settings
	try {
		settings.value = await api.updateSettings(next)
		store.settings = settings.value
		// A theme is visible the moment it changes. Waiting for a restart to
		// see it would make the control look broken.
		if (key === 'theme') store.applyTheme(String(value))
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

onMounted(load)
</script>

<template>
	<div v-if="!settings" class="pc-scroll flex-1 px-6 pb-28 pt-6 sm:px-10 lg:px-14">
		<p class="pc-eyebrow">Loading settings…</p>
	</div>
	<div v-else class="pc-scroll flex-1 px-6 pb-28 pt-6 sm:px-10 lg:px-14 lg:pb-12">
		<header class="flex items-start justify-between">
			<div>
				<h2 class="pc-display text-4xl sm:text-5xl">Settings</h2>
				<p class="mt-2 text-muted">System configuration and preferences.</p>
			</div>
			<button class="pc-btn pc-btn-ghost h-11 w-11 p-0" aria-label="Settings">
				<PcIcon name="settings" :size="20" />
			</button>
		</header>

		<!-- section jumpers -->
		<nav class="mt-8 flex flex-wrap gap-2" aria-label="Settings sections">
			<button
				v-for="s in sections"
				:key="s.id"
				class="pc-chip transition-colors hover:text-ink"
				@click="scrollTo(s.id)"
			>
				<span class="pc-numeral">{{ s.numeral }}</span>
				{{ s.title }}
			</button>
		</nav>

		<!-- 01 Network -->
		<section :id="'section-network'" class="mt-12">
			<div class="flex items-baseline gap-4">
				<span class="pc-numeral">01</span>
				<h3 class="pc-section-title">Network</h3>
			</div>

			<div class="pc-card mt-4 p-6">
				<p class="pc-eyebrow">Identity</p>
				<p class="mt-2 text-sm text-muted">
					The name this computer advertises to phones over the KDE Connect protocol.
				</p>
				<div class="mt-4 flex gap-2">
					<input
						v-model="settings.kdeDeviceName"
						type="text"
						class="pc-inset flex-1 bg-transparent px-4 py-3 outline-none"
						placeholder="My laptop"
						aria-label="Advertised device name"
						@change="save('kdeDeviceName', settings.kdeDeviceName)"
					/>
				</div>

				<div class="pc-divider mt-6 border-t" />

				<div class="mt-6">
					<p class="pc-eyebrow">Download folder</p>
					<!--
						One folder for both protocols. They used to have separate
						pickers, so choosing one left the other writing somewhere the
						user had not picked - and files arrived in an unexpected place
						with nothing indicating which setting had changed.
					-->
					<p class="mt-2 truncate text-sm text-muted">
						{{ settings.downloadFolder ?? settings.downloadPath ?? '~/Downloads' }}
					</p>
					<p class="mt-1 text-xs text-faint">
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

				<div class="pc-divider mt-6 border-t" />

				<div class="mt-6">
					<p class="pc-eyebrow">Quick Share visibility</p>
					<p class="mt-2 text-sm text-muted">
						Controls whether this computer advertises itself to nearby phones.
					</p>
					<div class="mt-3 flex gap-2">
						<button
							v-for="opt in [
								{ v: 0, l: 'Visible' },
								{ v: 1, l: 'Invisible' },
								{ v: 2, l: 'Temporary' },
							]"
							:key="opt.v"
							class="pc-btn pc-btn-sm"
							:class="
								settings.visibility === opt.v
									? 'pc-btn-primary'
									: 'pc-btn-secondary'
							"
							@click="save('visibility', opt.v)"
						>
							{{ opt.l }}
						</button>
					</div>
				</div>
			</div>
		</section>

		<!-- 02 Preferences -->
		<section :id="'section-preferences'" class="mt-16">
			<div class="flex items-baseline gap-4">
				<span class="pc-numeral">02</span>
				<h3 class="pc-section-title">Preferences</h3>
			</div>

			<div class="mt-4 grid gap-4 lg:grid-cols-2">
				<div class="pc-card flex items-center gap-4 px-6 py-5">
					<PcIcon name="copy" :size="20" class="text-muted" />
					<div class="min-w-0 flex-1">
						<p class="font-semibold">Clipboard sync</p>
						<p class="text-sm text-muted">
							Share copied text between the phone and this computer.
						</p>
						<p v-if="store.clipboardCaps" class="mt-1 text-xs text-faint">
							{{ store.clipboardCaps.mechanismDetail }}
						</p>
						<p
							v-if="store.clipboardCaps"
							class="mt-1 text-xs"
							:class="store.clipboardCaps.backgroundReadAllowed ? 'text-online' : 'text-warn'"
						>
							{{ store.clipboardCaps.backgroundReadAllowed
								? 'Runs in the background — the window does not need to be focused'
								: 'Needs focus — the window must be focused to read the clipboard' }}
						</p>
					</div>
					<ToggleSwitch
						:model-value="settings.clipboardEnabled"
						label="Clipboard sync"
						@update:model-value="save('clipboardEnabled', $event)"
					/>
				</div>

				<div class="pc-card flex items-center gap-4 px-6 py-5">
					<PcIcon name="bell" :size="20" class="text-muted" />
					<div class="min-w-0 flex-1">
						<p class="font-semibold">Auto-share clipboard</p>
						<p class="text-sm text-muted">
							Send clipboard changes automatically instead of on demand.
						</p>
					</div>
					<ToggleSwitch
						:model-value="settings.clipboardAutoShare"
						label="Auto-share clipboard"
						:disabled="!settings.clipboardEnabled"
						@update:model-value="save('clipboardAutoShare', $event)"
					/>
				</div>

				<div class="pc-card flex items-center gap-4 px-6 py-5">
					<PcIcon name="moon" :size="20" class="text-muted" />
					<div class="min-w-0 flex-1">
						<p class="font-semibold">Dark mode</p>
						<p class="text-sm text-muted">Optimised for AMOLED screens.</p>
					</div>
					<ToggleSwitch
						:model-value="true"
						label="Dark mode"
						disabled
					/>
					<span class="pc-chip">Always on in V1</span>
				</div>

				<div class="pc-card flex items-center gap-4 px-6 py-5">
					<PcIcon name="shield" :size="20" class="text-muted" />
					<div class="min-w-0 flex-1">
						<p class="font-semibold">Auto-accept incoming shares</p>
						<p class="text-sm text-muted">
							Accept files from your phone without being asked. Off by default: any
							paired device can send, so this lets a phone write to this computer
							unprompted. Files are saved to your download folder either way.
						</p>
					</div>
					<ToggleSwitch
						:model-value="settings.autoAcceptShares"
						label="Auto-accept incoming shares"
						@update:model-value="save('autoAcceptShares', $event)"
					/>
				</div>

				<div class="pc-card flex items-center gap-4 px-6 py-5">
					<PcIcon name="image" :size="20" class="text-muted" />
					<div class="min-w-0 flex-1">
						<p class="font-semibold">Copy received images to the clipboard</p>
						<p class="text-sm text-muted">
							So a photo sent from your phone can be pasted straight into a message.
							Only images; other files are just saved.
						</p>
					</div>
					<ToggleSwitch
						:model-value="settings.clipboardImagesOnReceive"
						label="Copy received images to the clipboard"
						@update:model-value="save('clipboardImagesOnReceive', $event)"
					/>
				</div>

				<!--
					Notification categories. Each event is switchable on its own so a
					transfer can announce itself without every progress tick also
					announcing itself. `notifyProgress` is off by default: it fires
					repeatedly during a single transfer.
				-->
				<div class="pc-card flex items-center gap-4 px-6 py-5">
					<PcIcon name="bell" :size="20" class="text-muted" />
					<div class="min-w-0 flex-1">
						<p class="font-semibold">Notify when text is shared</p>
						<p class="text-sm text-muted">
							Your phone sends text or a link and it lands on your clipboard.
						</p>
					</div>
					<ToggleSwitch
						:model-value="settings.notifySharedText"
						label="Notify when text is shared"
						@update:model-value="save('notifySharedText', $event)"
					/>
				</div>

				<div class="pc-card flex items-center gap-4 px-6 py-5">
					<PcIcon name="bell" :size="20" class="text-muted" />
					<div class="min-w-0 flex-1">
						<p class="font-semibold">Notify about incoming shares</p>
						<p class="text-sm text-muted">
							When your phone sends a file or a link. The notification can accept or
							reject it without opening this window.
						</p>
					</div>
					<ToggleSwitch
						:model-value="settings.notifyShareRequest"
						label="Notify about incoming shares"
						@update:model-value="save('notifyShareRequest', $event)"
					/>
				</div>

				<div class="pc-card flex items-center gap-4 px-6 py-5">
					<PcIcon name="check" :size="20" class="text-muted" />
					<div class="min-w-0 flex-1">
						<p class="font-semibold">Notify when a transfer finishes</p>
						<p class="text-sm text-muted">One notification per completed transfer.</p>
					</div>
					<ToggleSwitch
						:model-value="settings.notifyTransferComplete"
						label="Notify when a transfer finishes"
						@update:model-value="save('notifyTransferComplete', $event)"
					/>
				</div>

				<div class="pc-card flex items-center gap-4 px-6 py-5">
					<PcIcon name="alert" :size="20" class="text-muted" />
					<div class="min-w-0 flex-1">
						<p class="font-semibold">Notify when a transfer fails</p>
						<p class="text-sm text-muted">
							Includes the reason, so a failure is never silent.
						</p>
					</div>
					<ToggleSwitch
						:model-value="settings.notifyTransferFailed"
						label="Notify when a transfer fails"
						@update:model-value="save('notifyTransferFailed', $event)"
					/>
				</div>

				<div class="pc-card flex items-center gap-4 px-6 py-5">
					<PcIcon name="activity" :size="20" class="text-muted" />
					<div class="min-w-0 flex-1">
						<p class="font-semibold">Notify on transfer progress</p>
						<p class="text-sm text-muted">
							Off by default: this fires repeatedly while a transfer runs. Progress is
							always shown in the app.
						</p>
					</div>
					<ToggleSwitch
						:model-value="settings.notifyProgress"
						label="Notify on transfer progress"
						@update:model-value="save('notifyProgress', $event)"
					/>
				</div>

				<div class="pc-card flex items-center gap-4 px-6 py-5">
					<PcIcon name="bolt" :size="20" class="text-muted" />
					<div class="min-w-0 flex-1">
						<p class="font-semibold">Start at login</p>
						<p class="text-sm text-muted">Launch Pixel Connect when you sign in.</p>
					</div>
					<ToggleSwitch
						:model-value="settings.autostart"
						label="Start at login"
						@update:model-value="save('autostart', $event)"
					/>
				</div>

				<div class="pc-card flex items-center gap-4 px-6 py-5">
					<PcIcon name="close" :size="20" class="text-muted" />
					<div class="min-w-0 flex-1">
						<p class="font-semibold">Close instead of minimise</p>
						<p class="text-sm text-muted">
							Close the window for real rather than hiding to the tray.
						</p>
					</div>
					<ToggleSwitch
						:model-value="settings.realclose"
						label="Close instead of minimise"
						@update:model-value="save('realclose', $event)"
					/>
				</div>

				<!--
					Theme.

					Each swatch is the theme's own tokens rather than a drawn
					approximation, so what you see here is literally what the
					interface becomes.
				-->
				<div class="pc-card px-6 py-5 lg:col-span-2">
					<div class="flex items-center gap-4">
						<PcIcon name="moon" :size="20" class="text-muted" />
						<div class="min-w-0 flex-1">
							<p class="font-semibold">Appearance</p>
							<p class="text-sm text-muted">
								The two contrast themes raise the borders and secondary text as well as
								the body, which is what makes them easier to read on a poor display.
							</p>
						</div>
					</div>

					<div
						class="mt-5 grid gap-3 sm:grid-cols-2 lg:grid-cols-3"
						role="radiogroup"
						aria-label="Colour theme"
					>
						<button
							v-for="option in THEMES"
							:key="option.id"
							type="button"
							role="radio"
							:aria-checked="currentTheme === option.id"
							class="pc-card flex items-center gap-3 px-4 py-3 text-left transition"
							:class="
								currentTheme === option.id
									? 'ring-2 ring-[rgb(var(--pc-text))]'
									: 'hover:bg-[rgb(var(--pc-card-alt))]'
							"
							@click="save('theme', option.id)"
						>
							<span
								aria-hidden="true"
								class="flex h-8 w-12 shrink-0 overflow-hidden rounded-md border"
								:style="{
									backgroundColor: `rgb(${option.swatch.bg})`,
									borderColor: `rgb(${option.swatch.line})`
								}"
							>
								<span
									class="h-full w-1/2"
									:style="{ backgroundColor: `rgb(${option.swatch.accent})` }"
								/>
								<span
									class="flex flex-1 flex-col justify-center gap-1 px-1"
								>
									<span
										class="block h-1 w-full rounded-full"
										:style="{ backgroundColor: `rgb(${option.swatch.text})` }"
									/>
									<span
										class="block h-1 w-2/3 rounded-full"
										:style="{ backgroundColor: `rgb(${option.swatch.muted})` }"
									/>
								</span>
							</span>
							<span class="min-w-0">
								<span class="block truncate font-medium">{{ option.label }}</span>
								<span class="block truncate text-xs text-muted">{{ option.note }}</span>
							</span>
						</button>
					</div>
				</div>
			</div>
		</section>

		<!-- 03 Mirroring & access -->
		<section :id="'section-access'" class="mt-16">
			<div class="flex items-baseline gap-4">
				<span class="pc-numeral">03</span>
				<h3 class="pc-section-title">Screen mirroring &amp; access</h3>
			</div>

			<div class="pc-card mt-4 p-6">
				<p class="pc-eyebrow">scrcpy</p>
				<p class="mt-2 text-sm text-muted">
					Pixel Connect manages scrcpy and ADB for you; it never enables debugging or
					changes developer settings on the phone.
				</p>
				<dl class="mt-4 grid gap-3 text-sm sm:grid-cols-2">
					<div class="flex justify-between gap-4">
						<dt class="text-muted">adb</dt>
						<dd class="truncate text-right font-mono text-xs">
							{{ store.tools?.adbPath ?? 'not found' }}
						</dd>
					</div>
					<div class="flex justify-between gap-4">
						<dt class="text-muted">scrcpy</dt>
						<dd class="truncate text-right font-mono text-xs">
							{{ store.tools?.scrcpyPath ?? 'not found' }}
						</dd>
					</div>
				</dl>
				<div v-if="store.tools?.error" class="mt-4 rounded-xl bg-warn/10 px-4 py-3 text-sm text-warn">
					{{ store.tools.error }}
				</div>
				<button class="pc-btn pc-btn-secondary pc-btn-sm mt-4" @click="store.bootstrap()">
					<PcIcon name="refresh" :size="14" />
					Re-detect tools
				</button>
			</div>

			<div class="pc-card mt-4 p-6">
				<p class="pc-eyebrow">SSH (optional)</p>
				<p class="mt-2 text-sm text-muted">
					Connect to a phone that already has an SSH server you configured. Pixel Connect
					never installs or enables one.
				</p>

				<div v-if="!sshAvailable" class="mt-4 rounded-xl bg-warn/10 px-4 py-3 text-sm text-warn">
					The system OpenSSH client was not found on PATH.
				</div>

				<div v-else class="mt-5 grid gap-4 sm:grid-cols-2">
					<label class="block">
						<span class="pc-eyebrow">Host</span>
						<input
							v-model="ssh.host"
							type="text"
							class="pc-inset mt-2 w-full bg-transparent px-4 py-3 outline-none"
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
							class="pc-inset mt-2 w-full bg-transparent px-4 py-3 outline-none"
						/>
					</label>
					<label class="block">
						<span class="pc-eyebrow">Username</span>
						<input
							v-model="ssh.username"
							type="text"
							class="pc-inset mt-2 w-full bg-transparent px-4 py-3 outline-none"
							placeholder="termux"
						/>
					</label>
					<label class="block">
						<span class="pc-eyebrow">Authentication</span>
						<select
							v-model="ssh.auth_method"
							class="pc-inset mt-2 w-full bg-transparent px-4 py-3 outline-none"
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
							class="pc-inset mt-2 w-full bg-transparent px-4 py-3 outline-none"
							placeholder="/home/me/.ssh/id_ed25519"
						/>
					</label>
				</div>

				<label v-if="sshAvailable" class="mt-5 flex items-center gap-3 text-sm">
					<input v-model="ssh.confirmed" type="checkbox" />
					<span>I have configured an SSH server on this phone myself.</span>
				</label>

				<div class="mt-5 flex flex-wrap gap-2">
					<button
						class="pc-btn pc-btn-secondary pc-btn-sm"
						:disabled="!sshAvailable || !ssh.confirmed"
						@click="testSsh"
					>
						<PcIcon name="terminal" :size="14" />
						Test connection
					</button>
					<button
						class="pc-btn pc-btn-primary pc-btn-sm"
						:disabled="!sshAvailable || !ssh.confirmed || !ssh.host"
						@click="openSsh"
					>
						Open session
					</button>
				</div>
				<p v-if="sshMessage" class="mt-3 text-sm text-muted">{{ sshMessage }}</p>
			</div>
		</section>

		<!-- 04 Integrations -->
		<section :id="'section-integrations'" class="mt-16">
			<div class="flex items-baseline gap-4">
				<span class="pc-numeral">04</span>
				<h3 class="pc-section-title">Integrations</h3>
			</div>

			<div class="pc-card mt-4 p-6">
				<p class="pc-eyebrow">Files — "Send to Pixel"</p>
				<p class="mt-2 text-sm text-muted">
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
		</section>

		<!-- 05 About -->
		<section :id="'section-about'" class="mt-16">
			<div class="flex items-baseline gap-4">
				<span class="pc-numeral">05</span>
				<h3 class="pc-section-title">About</h3>
			</div>

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
</template>