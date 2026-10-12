<script setup lang="ts">
// Pixel Connect — number stepper replacing the native spinner.
//
// -/+ steppers around a spinbutton readout; value clamps to min/max and
// snaps to step. Every target is >= 40px.

import { computed, useId } from 'vue'

const props = withDefaults(
	defineProps<{
		modelValue: number
		min?: number
		max?: number
		step?: number
		disabled?: boolean
		label?: string
	}>(),
	{ min: undefined, max: undefined, step: 1, disabled: false, label: 'Value' },
)

const emit = defineEmits<{ 'update:modelValue': [number] }>()

const id = useId()

function clamp(v: number): number {
	let out = v
	if (props.min !== undefined) out = Math.max(props.min, out)
	if (props.max !== undefined) out = Math.min(props.max, out)
	// Snap to step from min (or 0) to avoid float drift.
	const base = props.min ?? 0
	out = base + Math.round((out - base) / props.step) * props.step
	// Round float artefacts (e.g. 0.1 + 0.2).
	out = Math.round(out * 1e6) / 1e6
	if (props.min !== undefined) out = Math.max(props.min, out)
	if (props.max !== undefined) out = Math.min(props.max, out)
	return out
}

const canDec = computed(() => !props.disabled && (props.min === undefined || props.modelValue > props.min))
const canInc = computed(() => !props.disabled && (props.max === undefined || props.modelValue < props.max))

function dec() {
	if (canDec.value) emit('update:modelValue', clamp(props.modelValue - props.step))
}

function inc() {
	if (canInc.value) emit('update:modelValue', clamp(props.modelValue + props.step))
}

function onKey(e: KeyboardEvent) {
	if (props.disabled) return
	if (e.key === 'ArrowUp' || e.key === 'ArrowRight') {
		e.preventDefault()
		inc()
	} else if (e.key === 'ArrowDown' || e.key === 'ArrowLeft') {
		e.preventDefault()
		dec()
	} else if (e.key === 'Home' && props.min !== undefined) {
		e.preventDefault()
		emit('update:modelValue', props.min)
	} else if (e.key === 'End' && props.max !== undefined) {
		e.preventDefault()
		emit('update:modelValue', props.max)
	}
}
</script>

<template>
	<div class="m3step" :class="{ 'm3step--disabled': disabled }">
		<button
			type="button"
			class="m3step__btn"
			aria-label="Decrease value"
			:disabled="!canDec"
			@click="dec"
		>
			<span class="m3step__minus" aria-hidden="true" />
		</button>
		<span
			:id="id"
			class="m3step__value"
			role="spinbutton"
			tabindex="0"
			:aria-label="label"
			:aria-valuenow="modelValue"
			:aria-valuemin="min"
			:aria-valuemax="max"
			@keydown="onKey"
		>
			{{ modelValue }}
		</span>
		<button
			type="button"
			class="m3step__btn"
			aria-label="Increase value"
			:disabled="!canInc"
			@click="inc"
		>
			<span class="m3step__plus" aria-hidden="true" />
		</button>
	</div>
</template>

<style scoped>
.m3step {
	display: inline-flex;
	align-items: center;
	gap: 0.25rem;
	padding: 0.25rem;
	border-radius: var(--pc-radius-pill);
	border: 1px solid rgb(var(--pc-outline));
	background-color: transparent;
}

.m3step__btn {
	display: flex;
	align-items: center;
	justify-content: center;
	width: 40px;
	height: 40px;
	border-radius: 9999px;
	color: rgb(var(--pc-text));
	cursor: pointer;
	transition:
		transform var(--pc-duration-shortest) var(--pc-ease-emphasized),
		background-color var(--pc-duration-shortest) var(--pc-ease-emphasized),
		opacity var(--pc-duration-short) var(--pc-ease-emphasized);
}

.m3step__btn:hover:not(:disabled) {
	background-color: rgb(var(--pc-card-alt));
}

.m3step__btn:focus-visible {
	outline: 2px solid var(--pc-primary);
	outline-offset: 2px;
}

.m3step__btn:active:not(:disabled) {
	transform: scale(0.97);
}

.m3step__btn:disabled {
	opacity: 0.35;
	cursor: not-allowed;
}

.m3step__minus {
	display: block;
	width: 1rem;
	height: 2px;
	border-radius: 2px;
	background: currentColor;
}

.m3step__plus {
	position: relative;
	display: block;
	width: 1rem;
	height: 1rem;
}

.m3step__plus::before,
.m3step__plus::after {
	content: '';
	position: absolute;
	border-radius: 2px;
	background: currentColor;
}

.m3step__plus::before {
	top: 50%;
	right: 0;
	left: 0;
	height: 2px;
	margin-top: -1px;
}

.m3step__plus::after {
	top: 0;
	bottom: 0;
	left: 50%;
	width: 2px;
	margin-left: -1px;
}

.m3step__value {
	min-width: 3rem;
	padding: 0 0.5rem;
	border-radius: 0.5rem;
	text-align: center;
	font-size: 1rem;
	font-weight: 600;
	font-variant-numeric: tabular-nums;
	color: rgb(var(--pc-text));
}

.m3step__value:focus-visible {
	outline: 2px solid var(--pc-primary);
	outline-offset: 2px;
}

.m3step--disabled {
	opacity: 0.4;
}

/* Reduced motion: fades cap at 100ms, no scale. */
@media (prefers-reduced-motion: reduce) {
	.m3step__btn {
		transition-duration: var(--pc-duration-shortest);
	}
	.m3step__btn:active:not(:disabled) {
		transform: none;
	}
}

html[data-reduce-motion] .m3step__btn {
	transition-duration: var(--pc-duration-shortest);
}

html[data-reduce-motion] .m3step__btn:active:not(:disabled) {
	transform: none;
}
</style>
