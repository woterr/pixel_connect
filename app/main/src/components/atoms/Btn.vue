<script setup lang="ts">
// Pixel Connect — Material 3 Expressive button.
//
// Variants: filled / tonal / outlined / text. Ripple originates at the
// pointer, press scales to .97, state layers are 8/10/10%, keyboard focus
// gets a 2px accent ring offset 2px, every target is >= 40px, and the error
// tone adds a shake entrance.

import { computed, ref } from 'vue'

type Variant = 'filled' | 'tonal' | 'outlined' | 'text'

const props = withDefaults(
	defineProps<{
		variant?: Variant
		disabled?: boolean
		type?: 'button' | 'submit' | 'reset'
		loading?: boolean
		/** Error tone with a shake entrance. */
		error?: boolean
	}>(),
	{ variant: 'filled', disabled: false, type: 'button', loading: false, error: false },
)

defineEmits<{ click: [MouseEvent] }>()

interface Ripple {
	id: number
	x: number
	y: number
}

let nextId = 0
const ripples = ref<Ripple[]>([])

const isDisabled = computed(() => props.disabled || props.loading)

function spawnRipple(e: PointerEvent) {
	if (isDisabled.value) return
	const el = e.currentTarget as HTMLElement | null
	if (!el) return
	const rect = el.getBoundingClientRect()
	ripples.value.push({ id: nextId++, x: e.clientX - rect.left, y: e.clientY - rect.top })
	if (ripples.value.length > 3) ripples.value.shift()
}

function clearRipple(id: number) {
	ripples.value = ripples.value.filter((r) => r.id !== id)
}
</script>

<template>
	<button
		:type="type"
		class="m3btn"
		:class="[`m3btn--${variant}`, { 'm3btn--error': error }]"
		:disabled="isDisabled"
		:aria-busy="loading || undefined"
		@click="$emit('click', $event)"
		@pointerdown="spawnRipple"
	>
		<span v-if="loading" class="m3btn__spinner" aria-hidden="true" />
		<slot />
		<span class="m3btn__ripples" aria-hidden="true">
			<span
				v-for="r in ripples"
				:key="r.id"
				class="m3btn__ripple"
				:style="{ left: `${r.x}px`, top: `${r.y}px` }"
				@animationend="clearRipple(r.id)"
			/>
		</span>
	</button>
</template>

<style scoped>
.m3btn {
	position: relative;
	display: inline-flex;
	align-items: center;
	justify-content: center;
	gap: 0.5rem;
	min-height: 40px;
	min-width: 40px;
	padding: 0.625rem 1.5rem;
	border-radius: var(--pc-radius-pill);
	border: 1px solid transparent;
	font-weight: 600;
	font-size: 0.9375rem;
	line-height: 1.25rem;
	cursor: pointer;
	overflow: hidden;
	isolation: isolate;
	transition:
		transform var(--pc-duration-shortest) var(--pc-ease-emphasized),
		background-color var(--pc-duration-short) var(--pc-ease-emphasized),
		color var(--pc-duration-short) var(--pc-ease-emphasized),
		border-color var(--pc-duration-short) var(--pc-ease-emphasized),
		opacity var(--pc-duration-short) var(--pc-ease-emphasized);
}

/* State layer: hover 8%, focus 10%, pressed 10%. */
.m3btn::after {
	content: '';
	position: absolute;
	inset: 0;
	border-radius: inherit;
	background: currentColor;
	opacity: 0;
	transition: opacity var(--pc-duration-shortest) var(--pc-ease-emphasized);
	pointer-events: none;
}

.m3btn:hover:not(:disabled)::after {
	opacity: 0.08;
}

.m3btn:focus-visible {
	outline: 2px solid var(--pc-primary);
	outline-offset: 2px;
}

.m3btn:focus-visible::after {
	opacity: 0.1;
}

.m3btn:active:not(:disabled) {
	transform: scale(0.97);
}

.m3btn:active:not(:disabled)::after {
	opacity: 0.1;
}

.m3btn:disabled {
	opacity: 0.4;
	cursor: not-allowed;
}

.m3btn--filled {
	background-color: var(--pc-primary);
	color: var(--pc-on-primary);
}

.m3btn--tonal {
	background-color: var(--pc-secondary-container);
	color: var(--pc-on-secondary-container);
}

.m3btn--outlined {
	background-color: transparent;
	border-color: rgb(var(--pc-outline));
	color: var(--pc-primary);
}

.m3btn--text {
	background-color: transparent;
	color: var(--pc-primary);
}

.m3btn--error {
	background-color: rgb(var(--pc-danger));
	border-color: transparent;
	color: #ffffff;
	animation: m3btn-shake var(--pc-duration-medium) var(--pc-ease-emphasized);
}

.m3btn--error.m3btn--outlined,
.m3btn--error.m3btn--text {
	background-color: transparent;
	color: rgb(var(--pc-danger));
}

@keyframes m3btn-shake {
	0%,
	100% {
		transform: translateX(0);
	}
	20% {
		transform: translateX(-5px);
	}
	40% {
		transform: translateX(5px);
	}
	60% {
		transform: translateX(-3px);
	}
	80% {
		transform: translateX(3px);
	}
}

.m3btn__ripples {
	position: absolute;
	inset: 0;
	overflow: hidden;
	border-radius: inherit;
	pointer-events: none;
}

.m3btn__ripple {
	position: absolute;
	width: 8px;
	height: 8px;
	margin: -4px 0 0 -4px;
	border-radius: 9999px;
	background: currentColor;
	animation: m3btn-ripple var(--pc-duration-medium) var(--pc-ease-decelerate) forwards;
}

@keyframes m3btn-ripple {
	from {
		transform: scale(1);
		opacity: 0.18;
	}
	to {
		transform: scale(28);
		opacity: 0;
	}
}

.m3btn__spinner {
	width: 1rem;
	height: 1rem;
	flex: none;
	border-radius: 9999px;
	border: 2px solid currentColor;
	border-top-color: transparent;
	animation: m3btn-spin var(--pc-duration-long) linear infinite;
}

@keyframes m3btn-spin {
	to {
		transform: rotate(360deg);
	}
}

/* Reduced motion: fades cap at 100ms, no translation or scale. */
@media (prefers-reduced-motion: reduce) {
	.m3btn {
		transition-duration: var(--pc-duration-shortest);
	}
	.m3btn:active:not(:disabled) {
		transform: none;
	}
	.m3btn__ripple,
	.m3btn__spinner,
	.m3btn--error {
		animation: none !important;
	}
}

html[data-reduce-motion] .m3btn {
	transition-duration: var(--pc-duration-shortest);
}

html[data-reduce-motion] .m3btn:active:not(:disabled) {
	transform: none;
}

html[data-reduce-motion] .m3btn__ripple,
html[data-reduce-motion] .m3btn__spinner,
html[data-reduce-motion] .m3btn--error {
	animation: none !important;
}
</style>
