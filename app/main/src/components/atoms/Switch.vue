<script setup lang="ts">
// Pixel Connect — Material 3 Expressive switch.
//
// Thumb grows on press, carries a check when on and a cross when off, and
// rides a gentle spring. Same props/events as the former ToggleSwitch, so it
// is a drop-in replacement. Reuses PcIcon only.

import PcIcon from './PcIcon.vue'

const props = withDefaults(
	defineProps<{
		modelValue: boolean
		label?: string
		disabled?: boolean
	}>(),
	{ label: undefined, disabled: false },
)

const emit = defineEmits<{ 'update:modelValue': [boolean] }>()

function toggle() {
	if (!props.disabled) emit('update:modelValue', !props.modelValue)
}
</script>

<template>
	<button
		type="button"
		role="switch"
		class="m3switch"
		:aria-checked="props.modelValue"
		:aria-label="props.label"
		:disabled="props.disabled"
		:data-on="props.modelValue"
		@click="toggle"
	>
		<span class="m3switch__track" aria-hidden="true">
			<span class="m3switch__thumb">
				<PcIcon v-if="props.modelValue" name="check" :size="14" />
				<PcIcon v-else name="close" :size="11" />
			</span>
		</span>
	</button>
</template>

<style scoped>
.m3switch {
	display: inline-flex;
	align-items: center;
	justify-content: center;
	min-width: 40px;
	min-height: 40px;
	padding: 4px;
	border-radius: var(--pc-radius-pill);
	cursor: pointer;
}

.m3switch:disabled {
	opacity: 0.4;
	cursor: not-allowed;
}

.m3switch:focus-visible {
	outline: 2px solid var(--pc-primary);
	outline-offset: 2px;
}

.m3switch__track {
	display: block;
	position: relative;
	width: 52px;
	height: 32px;
	border-radius: var(--pc-radius-pill);
	border: 2px solid rgb(var(--pc-outline));
	background-color: rgb(var(--pc-container-highest));
	transition:
		background-color var(--pc-duration-short) var(--pc-ease-emphasized),
		border-color var(--pc-duration-short) var(--pc-ease-emphasized);
}

.m3switch[data-on='true'] .m3switch__track {
	border-color: transparent;
	background-color: var(--pc-primary);
}

.m3switch__thumb {
	position: absolute;
	top: 50%;
	left: 4px;
	display: flex;
	align-items: center;
	justify-content: center;
	width: 20px;
	height: 20px;
	margin-top: -10px;
	border-radius: 9999px;
	background-color: rgb(var(--pc-muted));
	color: rgb(var(--pc-container-highest));
	transform: translateX(0);
	transition:
		transform var(--pc-duration-short) var(--pc-spring-snappy),
		width var(--pc-duration-short) var(--pc-spring-snappy),
		height var(--pc-duration-short) var(--pc-spring-snappy),
		margin-top var(--pc-duration-short) var(--pc-spring-snappy),
		background-color var(--pc-duration-short) var(--pc-ease-emphasized),
		color var(--pc-duration-short) var(--pc-ease-emphasized);
}

.m3switch[data-on='true'] .m3switch__thumb {
	width: 24px;
	height: 24px;
	margin-top: -12px;
	transform: translateX(20px);
	background-color: var(--pc-on-primary);
	color: var(--pc-primary);
}

/* Thumb grows while pressed; `scale` stays independent of the slide. */
.m3switch:active:not(:disabled) .m3switch__thumb {
	scale: 1.15;
}

/* Reduced motion: no slide or grow, quick fades only. */
@media (prefers-reduced-motion: reduce) {
	.m3switch__track,
	.m3switch__thumb {
		transition-duration: var(--pc-duration-shortest);
	}
	.m3switch__thumb,
	.m3switch[data-on='true'] .m3switch__thumb {
		transform: none;
		left: 4px;
	}
	.m3switch[data-on='true'] .m3switch__thumb {
		left: 24px;
	}
	.m3switch:active:not(:disabled) .m3switch__thumb {
		scale: none;
	}
}

html[data-reduce-motion] .m3switch__track,
html[data-reduce-motion] .m3switch__thumb {
	transition-duration: var(--pc-duration-shortest);
}

html[data-reduce-motion] .m3switch__thumb,
html[data-reduce-motion] .m3switch[data-on='true'] .m3switch__thumb {
	transform: none;
	left: 4px;
}

html[data-reduce-motion] .m3switch[data-on='true'] .m3switch__thumb {
	left: 24px;
}

html[data-reduce-motion] .m3switch:active:not(:disabled) .m3switch__thumb {
	scale: none;
}
</style>
