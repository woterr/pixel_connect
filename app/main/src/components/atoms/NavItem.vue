<script setup lang="ts">
// Pixel Connect — Material 3 Expressive navigation item.
//
// 48px row with a sliding pill indicator, a stronger icon stroke when
// selected, and arrow-key navigation between sibling items. Reuses PcIcon
// only (stroke widens when selected; no second icon set).

import PcIcon from './PcIcon.vue'

const props = withDefaults(
	defineProps<{
		label: string
		icon: string
		selected?: boolean
		disabled?: boolean
		/** Accessible name override; defaults to the label. */
		ariaLabel?: string
	}>(),
	{ selected: false, disabled: false, ariaLabel: undefined },
)

const emit = defineEmits<{ click: [MouseEvent] }>()

/** Move focus across sibling [data-m3nav] items (ArrowUp/Down/Home/End). */
function onKey(e: KeyboardEvent) {
	if (!['ArrowUp', 'ArrowDown', 'Home', 'End'].includes(e.key)) return
	const current = e.currentTarget as HTMLElement
	const scope =
		current.closest('[data-m3nav-group]') ?? current.parentElement ?? document.body
	const items = [...scope.querySelectorAll<HTMLElement>('[data-m3nav]')].filter(
		(el) => !(el as HTMLButtonElement).disabled,
	)
	const at = items.indexOf(current)
	if (at < 0 || !items.length) return
	e.preventDefault()
	let next = at
	if (e.key === 'ArrowDown') next = (at + 1) % items.length
	else if (e.key === 'ArrowUp') next = (at - 1 + items.length) % items.length
	else if (e.key === 'Home') next = 0
	else if (e.key === 'End') next = items.length - 1
	items[next]?.focus()
}

function onClick(e: MouseEvent) {
	if (!props.disabled) emit('click', e)
}
</script>

<template>
	<button
		type="button"
		data-m3nav
		class="m3nav"
		:class="{ 'm3nav--selected': selected }"
		:aria-current="selected ? 'page' : undefined"
		:aria-label="ariaLabel ?? label"
		:disabled="disabled"
		@click="onClick"
		@keydown="onKey"
	>
		<span class="m3nav__pill" aria-hidden="true" />
		<span class="m3nav__content">
			<PcIcon :name="icon" :size="20" :stroke="selected ? 2.4 : 1.75" aria-hidden="true" />
			<span class="m3nav__label">{{ label }}</span>
			<span v-if="$slots.badge" class="m3nav__badge"><slot name="badge" /></span>
		</span>
	</button>
</template>

<style scoped>
.m3nav {
	position: relative;
	display: flex;
	align-items: center;
	width: 100%;
	min-height: 48px;
	padding: 0.375rem 1rem;
	border: 0;
	border-radius: var(--pc-radius-pill);
	background: transparent;
	color: rgb(var(--pc-muted));
	font-size: 0.9375rem;
	font-weight: 500;
	cursor: pointer;
	transition: color var(--pc-duration-short) var(--pc-ease-emphasized);
}

.m3nav:hover:not(:disabled) {
	color: rgb(var(--pc-text));
}

.m3nav:focus-visible {
	outline: 2px solid var(--pc-primary);
	outline-offset: 2px;
}

.m3nav:disabled {
	opacity: 0.4;
	cursor: not-allowed;
}

/* Sliding pill: scales in from the leading edge; switching items reads as
 * the indicator travelling. */
.m3nav__pill {
	position: absolute;
	inset: 0;
	border-radius: inherit;
	background-color: var(--pc-secondary-container);
	transform: scaleX(0);
	transform-origin: left center;
	opacity: 0;
	transition:
		transform var(--pc-duration-medium) var(--pc-spring-soft),
		opacity var(--pc-duration-short) var(--pc-ease-emphasized);
	pointer-events: none;
}

.m3nav--selected .m3nav__pill {
	transform: scaleX(1);
	opacity: 1;
}

.m3nav--selected {
	color: var(--pc-on-secondary-container);
	font-weight: 600;
}

.m3nav__content {
	position: relative;
	display: flex;
	align-items: center;
	gap: 0.75rem;
	width: 100%;
}

.m3nav__label {
	flex: 1 1 auto;
	text-align: left;
	overflow: hidden;
	text-overflow: ellipsis;
	white-space: nowrap;
}

.m3nav__badge {
	display: flex;
	flex: none;
}

/* Reduced motion: pill appears, fades cap at 100ms. */
@media (prefers-reduced-motion: reduce) {
	.m3nav,
	.m3nav__pill {
		transition-duration: var(--pc-duration-shortest);
	}
	.m3nav__pill {
		transform: none;
	}
}

html[data-reduce-motion] .m3nav,
html[data-reduce-motion] .m3nav__pill {
	transition-duration: var(--pc-duration-shortest);
}

html[data-reduce-motion] .m3nav__pill {
	transform: none;
}
</style>
