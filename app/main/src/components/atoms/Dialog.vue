<script setup lang="ts">
// Pixel Connect — Material 3 Expressive dialog.
//
// Escape closes, focus is trapped while open and restored on close.
// Correct dialog ARIA (role, aria-modal, labelledby).

import { nextTick, onUnmounted, ref, watch } from 'vue'

import PcIcon from './PcIcon.vue'

const props = withDefaults(
	defineProps<{
		open: boolean
		title?: string
		labelledby?: string
		describedby?: string
		/** Show the dismiss affordance. */
		dismissible?: boolean
	}>(),
	{ title: undefined, labelledby: undefined, describedby: undefined, dismissible: true },
)

const emit = defineEmits<{ close: []; 'update:open': [boolean] }>()

const panelRef = ref<HTMLElement | null>(null)
let restoreTo: HTMLElement | null = null

function labelledId(): string | undefined {
	if (props.labelledby) return props.labelledby
	return props.title ? 'm3dialog-title' : undefined
}

function requestClose() {
	emit('close')
	emit('update:open', false)
}

function focusables(): HTMLElement[] {
	if (!panelRef.value) return []
	return [
		...panelRef.value.querySelectorAll<HTMLElement>(
			'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
		),
	].filter((el) => el.offsetParent !== null || el === document.activeElement)
}

function onKey(e: KeyboardEvent) {
	if (!props.open) return
	if (e.key === 'Escape') {
		e.preventDefault()
		requestClose()
		return
	}
	if (e.key !== 'Tab') return
	const items = focusables()
	if (!items.length) {
		e.preventDefault()
		return
	}
	const first = items[0]!
	const last = items[items.length - 1]!
	if (e.shiftKey && document.activeElement === first) {
		e.preventDefault()
		last.focus()
	} else if (!e.shiftKey && document.activeElement === last) {
		e.preventDefault()
		first.focus()
	}
}

watch(
	() => props.open,
	(isOpen) => {
		if (isOpen) {
			restoreTo = document.activeElement as HTMLElement | null
			document.addEventListener('keydown', onKey, true)
			nextTick(() => {
				const items = focusables()
				;(items[0] ?? panelRef.value)?.focus()
			})
		} else {
			document.removeEventListener('keydown', onKey, true)
			restoreTo?.focus?.()
			restoreTo = null
		}
	},
	{ immediate: true },
)

onUnmounted(() => document.removeEventListener('keydown', onKey, true))
</script>

<template>
	<Teleport to="body">
		<div v-if="open" class="m3dlg" @pointerdown.self="dismissible && requestClose()">
			<div
				ref="panelRef"
				class="m3dlg__panel"
				role="dialog"
				aria-modal="true"
				:aria-labelledby="labelledId()"
				:aria-describedby="describedby"
				:aria-label="!labelledId() ? 'Dialog' : undefined"
				tabindex="-1"
			>
				<div v-if="title || dismissible" class="m3dlg__head">
					<h2 v-if="title" id="m3dialog-title" class="m3dlg__title">{{ title }}</h2>
					<button
						v-if="dismissible"
						type="button"
						class="m3dlg__dismiss"
						aria-label="Close dialog"
						@click="requestClose"
					>
						<PcIcon name="close" :size="18" />
					</button>
				</div>
				<div class="m3dlg__body"><slot /></div>
				<div v-if="$slots.actions" class="m3dlg__actions">
					<slot name="actions" />
				</div>
			</div>
		</div>
	</Teleport>
</template>

<style scoped>
.m3dlg {
	position: fixed;
	inset: 0;
	z-index: 100;
	display: flex;
	align-items: center;
	justify-content: center;
	padding: 1.5rem;
	background-color: rgb(0 0 0 / 0.55);
	animation: m3dlg-fade var(--pc-duration-short) var(--pc-ease-decelerate);
}

@keyframes m3dlg-fade {
	from {
		opacity: 0;
	}
	to {
		opacity: 1;
	}
}

.m3dlg__panel {
	width: min(100%, 28rem);
	max-height: calc(100vh - 3rem);
	overflow-y: auto;
	border-radius: var(--pc-radius-card);
	background-color: rgb(var(--pc-raised));
	color: rgb(var(--pc-text));
	box-shadow: 0 24px 64px rgb(0 0 0 / 0.5);
	animation: m3dlg-in var(--pc-duration-short) var(--pc-ease-decelerate);
}

@keyframes m3dlg-in {
	from {
		opacity: 0;
		transform: scale(0.95) translateY(8px);
	}
	to {
		opacity: 1;
		transform: scale(1) translateY(0);
	}
}

.m3dlg__panel:focus {
	outline: none;
}

.m3dlg__panel:focus-visible {
	outline: 2px solid var(--pc-primary);
	outline-offset: 2px;
}

.m3dlg__head {
	display: flex;
	align-items: center;
	gap: 0.75rem;
	padding: 1.25rem 1.25rem 0;
}

.m3dlg__title {
	flex: 1 1 auto;
	min-width: 0;
	margin: 0;
	font-size: 1.25rem;
	line-height: 1.75rem;
	font-weight: 600;
}

.m3dlg__dismiss {
	display: flex;
	align-items: center;
	justify-content: center;
	flex: none;
	width: 40px;
	height: 40px;
	border-radius: 9999px;
	color: rgb(var(--pc-muted));
	cursor: pointer;
	transition:
		background-color var(--pc-duration-shortest) var(--pc-ease-emphasized),
		color var(--pc-duration-shortest) var(--pc-ease-emphasized);
}

.m3dlg__dismiss:hover {
	background-color: rgb(var(--pc-card-alt));
	color: rgb(var(--pc-text));
}

.m3dlg__dismiss:focus-visible {
	outline: 2px solid var(--pc-primary);
	outline-offset: 2px;
}

.m3dlg__body {
	padding: 1rem 1.25rem 1.25rem;
	font-size: 0.9375rem;
	line-height: 1.5rem;
	color: rgb(var(--pc-muted));
}

.m3dlg__actions {
	display: flex;
	justify-content: flex-end;
	gap: 0.5rem;
	padding: 0 1.25rem 1.25rem;
}

/* Reduced motion: quick fades, no scale/translation. */
@media (prefers-reduced-motion: reduce) {
	.m3dlg,
	.m3dlg__panel {
		animation-duration: var(--pc-duration-shortest);
	}
	.m3dlg__panel {
		animation-name: m3dlg-fade;
	}
}

html[data-reduce-motion] .m3dlg,
html[data-reduce-motion] .m3dlg__panel {
	animation-duration: var(--pc-duration-shortest);
}

html[data-reduce-motion] .m3dlg__panel {
	animation-name: m3dlg-fade;
}
</style>
