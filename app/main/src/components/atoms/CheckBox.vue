<script setup lang="ts">
// Pixel Connect — custom checkbox replacing the native control.
//
// Animated check pop, mixed state for partial selection, 40px hit area,
// correct checkbox ARIA. Reuses PcIcon only.

import { computed } from 'vue'

import PcIcon from './PcIcon.vue'

const props = withDefaults(
	defineProps<{
		modelValue: boolean
		label?: string
		disabled?: boolean
		indeterminate?: boolean
	}>(),
	{ label: undefined, disabled: false, indeterminate: false },
)

const emit = defineEmits<{ 'update:modelValue': [boolean] }>()

const ariaChecked = computed(() => (props.indeterminate ? 'mixed' : props.modelValue))

function toggle() {
	if (!props.disabled) emit('update:modelValue', !props.modelValue)
}
</script>

<template>
	<button
		type="button"
		role="checkbox"
		class="m3check"
		:class="{ 'm3check--on': modelValue || indeterminate }"
		:aria-checked="ariaChecked"
		:aria-label="label"
		:disabled="disabled"
		@click="toggle"
	>
		<span class="m3check__box" aria-hidden="true">
			<PcIcon v-if="!indeterminate" class="m3check__icon" name="check" :size="14" />
			<span v-else class="m3check__dash" />
		</span>
		<span v-if="label || $slots.default" class="m3check__label">
			<slot>{{ label }}</slot>
		</span>
	</button>
</template>

<style scoped>
.m3check {
	display: inline-flex;
	align-items: center;
	gap: 0.625rem;
	min-height: 40px;
	min-width: 40px;
	border-radius: 0.5rem;
	cursor: pointer;
}

.m3check:disabled {
	opacity: 0.4;
	cursor: not-allowed;
}

.m3check:focus-visible {
	outline: 2px solid var(--pc-primary);
	outline-offset: 2px;
}

.m3check__box {
	display: flex;
	align-items: center;
	justify-content: center;
	width: 20px;
	height: 20px;
	flex: none;
	border-radius: 6px;
	border: 2px solid rgb(var(--pc-outline));
	background-color: transparent;
	color: transparent;
	transition:
		background-color var(--pc-duration-short) var(--pc-ease-emphasized),
		border-color var(--pc-duration-short) var(--pc-ease-emphasized),
		color var(--pc-duration-shortest) var(--pc-ease-emphasized),
		transform var(--pc-duration-shortest) var(--pc-ease-emphasized);
}

.m3check:hover:not(:disabled) .m3check__box {
	border-color: rgb(var(--pc-muted));
}

.m3check--on .m3check__box {
	border-color: transparent;
	background-color: var(--pc-primary);
	color: var(--pc-on-primary);
}

.m3check:active:not(:disabled) .m3check__box {
	transform: scale(0.9);
}

.m3check__icon {
	transform: scale(0);
	transition: transform var(--pc-duration-short) var(--pc-spring-snappy);
}

.m3check--on .m3check__icon {
	transform: scale(1);
}

.m3check__dash {
	display: block;
	width: 10px;
	height: 2px;
	border-radius: 2px;
	background: currentColor;
}

.m3check__label {
	font-size: 0.9375rem;
	line-height: 1.25rem;
	color: rgb(var(--pc-text));
	text-align: left;
}

/* Reduced motion: fades cap at 100ms, no scale. */
@media (prefers-reduced-motion: reduce) {
	.m3check__box,
	.m3check__icon {
		transition-duration: var(--pc-duration-shortest);
	}
	.m3check:active:not(:disabled) .m3check__box,
	.m3check__icon,
	.m3check--on .m3check__icon {
		transform: none;
	}
}

html[data-reduce-motion] .m3check__box,
html[data-reduce-motion] .m3check__icon {
	transition-duration: var(--pc-duration-shortest);
}

html[data-reduce-motion] .m3check:active:not(:disabled) .m3check__box,
html[data-reduce-motion] .m3check__icon,
html[data-reduce-motion] .m3check--on .m3check__icon {
	transform: none;
}
</style>
