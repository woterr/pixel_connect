<script setup lang="ts">
// Pixel Connect — Material 3 Expressive segmented buttons.
//
// Single-select group with a sliding pill indicator and arrow-key navigation.
// For Quick Share visibility and similar exclusive choices.

import { computed, ref } from 'vue'

import PcIcon from './PcIcon.vue'

export interface SegmentOption {
	value: string
	label: string
	icon?: string
	disabled?: boolean
}

const props = withDefaults(
	defineProps<{
		modelValue: string
		options: SegmentOption[]
		label?: string
		disabled?: boolean
	}>(),
	{ label: 'Options', disabled: false },
)

const emit = defineEmits<{ 'update:modelValue': [string] }>()

const selectedIndex = computed(() => {
	const i = props.options.findIndex((o) => o.value === props.modelValue)
	return i >= 0 ? i : 0
})

const btnRefs = ref<Array<HTMLButtonElement | null>>([])

function setBtnRef(i: number, el: unknown) {
	btnRefs.value[i] = el as HTMLButtonElement | null
}

function isEnabled(i: number): boolean {
	const o = props.options[i]
	return !!o && !o.disabled && !props.disabled
}

function select(i: number) {
	const o = props.options[i]
	if (!o || !isEnabled(i)) return
	if (o.value !== props.modelValue) emit('update:modelValue', o.value)
}

function step(from: number, dir: 1 | -1): number {
	const n = props.options.length
	for (let k = 1; k <= n; k++) {
		const i = (from + dir * k + n * k) % n
		if (isEnabled(i)) return i
	}
	return from
}

function onKey(e: KeyboardEvent) {
	const n = props.options.length
	if (n === 0) return
	let i = selectedIndex.value
	if (e.key === 'ArrowRight' || e.key === 'ArrowDown') i = step(i, 1)
	else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') i = step(i, -1)
	else if (e.key === 'Home') i = step(-1, 1)
	else if (e.key === 'End') i = step(n, -1)
	else return
	e.preventDefault()
	select(i)
	btnRefs.value[i]?.focus()
}
</script>

<template>
	<div
		class="m3seg"
		role="radiogroup"
		:aria-label="label"
		:style="{ '--m3seg-n': options.length, '--m3seg-i': selectedIndex }"
		@keydown="onKey"
	>
		<span class="m3seg__pill" aria-hidden="true" />
		<button
			v-for="(o, i) in options"
			:key="o.value"
			:ref="(el) => setBtnRef(i, el)"
			type="button"
			role="radio"
			class="m3seg__btn"
			:class="{ 'm3seg__btn--selected': i === selectedIndex }"
			:aria-checked="i === selectedIndex"
			:tabindex="i === selectedIndex ? 0 : -1"
			:disabled="disabled || o.disabled"
			@click="select(i)"
		>
			<PcIcon v-if="o.icon" :name="o.icon" :size="16" aria-hidden="true" />
			<span>{{ o.label }}</span>
		</button>
	</div>
</template>

<style scoped>
.m3seg {
	position: relative;
	display: flex;
	gap: 0;
	padding: 2px;
	border-radius: var(--pc-radius-pill);
	border: 1px solid rgb(var(--pc-outline));
	background-color: transparent;
}

.m3seg:focus-within {
	outline: 2px solid var(--pc-primary);
	outline-offset: 2px;
}

/* Sliding pill: width is one Nth of the track, translated by the index.
 * translateX % refers to the pill's own width, so this stays aligned. */
.m3seg__pill {
	position: absolute;
	top: 2px;
	bottom: 2px;
	left: 2px;
	width: calc((100% - 4px) / var(--m3seg-n, 1));
	border-radius: var(--pc-radius-pill);
	background-color: var(--pc-secondary-container);
	transform: translateX(calc(var(--m3seg-i, 0) * 100%));
	transition: transform var(--pc-duration-medium) var(--pc-spring-soft);
	pointer-events: none;
}

.m3seg__btn {
	position: relative;
	z-index: 1;
	flex: 1 1 0;
	display: inline-flex;
	align-items: center;
	justify-content: center;
	gap: 0.375rem;
	min-height: 40px;
	padding: 0.375rem 0.75rem;
	border-radius: var(--pc-radius-pill);
	font-size: 0.875rem;
	font-weight: 600;
	color: rgb(var(--pc-muted));
	cursor: pointer;
	transition:
		color var(--pc-duration-short) var(--pc-ease-emphasized),
		opacity var(--pc-duration-short) var(--pc-ease-emphasized);
}

.m3seg__btn:focus {
	outline: none;
}

.m3seg__btn:focus-visible {
	outline: 2px solid var(--pc-primary);
	outline-offset: 1px;
}

.m3seg__btn--selected {
	color: var(--pc-on-secondary-container);
}

.m3seg__btn:disabled {
	opacity: 0.4;
	cursor: not-allowed;
}

/* Reduced motion: pill jumps, fades cap at 100ms. */
@media (prefers-reduced-motion: reduce) {
	.m3seg__pill {
		transition: none;
	}
	.m3seg__btn {
		transition-duration: var(--pc-duration-shortest);
	}
}

html[data-reduce-motion] .m3seg__pill {
	transition: none;
}

html[data-reduce-motion] .m3seg__btn {
	transition-duration: var(--pc-duration-shortest);
}
</style>
