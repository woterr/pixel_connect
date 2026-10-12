<script setup lang="ts">
// Pixel Connect — Material 3 Expressive chip.
//
// Filter chips toggle with an animated selected check; assist chips are
// plain actions. Reuses PcIcon only.

import PcIcon from './PcIcon.vue'

const props = withDefaults(
	defineProps<{
		variant?: 'filter' | 'assist'
		selected?: boolean
		disabled?: boolean
		leadingIcon?: string
	}>(),
	{ variant: 'filter', selected: false, disabled: false, leadingIcon: undefined },
)

const emit = defineEmits<{ 'update:selected': [boolean]; click: [MouseEvent] }>()

function onClick(e: MouseEvent) {
	if (props.disabled) return
	if (props.variant === 'filter') emit('update:selected', !props.selected)
	emit('click', e)
}
</script>

<template>
	<button
		type="button"
		class="m3chip"
		:class="[`m3chip--${variant}`, { 'm3chip--selected': variant === 'filter' && selected }]"
		:disabled="disabled"
		:aria-pressed="variant === 'filter' ? selected : undefined"
		@click="onClick"
	>
		<span v-if="variant === 'filter'" class="m3chip__check" aria-hidden="true">
			<PcIcon name="check" :size="16" />
		</span>
		<PcIcon v-else-if="leadingIcon" :name="leadingIcon" :size="16" aria-hidden="true" />
		<span class="m3chip__label"><slot /></span>
	</button>
</template>

<style scoped>
.m3chip {
	display: inline-flex;
	align-items: center;
	gap: 0.375rem;
	min-height: 40px;
	padding: 0.375rem 1rem;
	border-radius: var(--pc-radius-pill);
	border: 1px solid rgb(var(--pc-outline));
	background-color: transparent;
	color: rgb(var(--pc-muted));
	font-size: 0.875rem;
	font-weight: 600;
	line-height: 1.25rem;
	cursor: pointer;
	transition:
		transform var(--pc-duration-shortest) var(--pc-ease-emphasized),
		background-color var(--pc-duration-short) var(--pc-ease-emphasized),
		color var(--pc-duration-short) var(--pc-ease-emphasized),
		border-color var(--pc-duration-short) var(--pc-ease-emphasized),
		opacity var(--pc-duration-short) var(--pc-ease-emphasized);
}

.m3chip:hover:not(:disabled) {
	background-color: rgb(var(--pc-card-alt));
	color: rgb(var(--pc-text));
}

.m3chip:focus-visible {
	outline: 2px solid var(--pc-primary);
	outline-offset: 2px;
}

.m3chip:active:not(:disabled) {
	transform: scale(0.97);
}

.m3chip:disabled {
	opacity: 0.4;
	cursor: not-allowed;
}

.m3chip--selected {
	background-color: var(--pc-secondary-container);
	border-color: transparent;
	color: var(--pc-on-secondary-container);
}

.m3chip--selected:hover:not(:disabled) {
	background-color: var(--pc-secondary-container);
	color: var(--pc-on-secondary-container);
	filter: brightness(1.08);
}

.m3chip__check {
	display: inline-flex;
	align-items: center;
	width: 0;
	opacity: 0;
	transform: scale(0);
	overflow: hidden;
	transition:
		width var(--pc-duration-short) var(--pc-spring-snappy),
		opacity var(--pc-duration-shortest) var(--pc-ease-emphasized),
		transform var(--pc-duration-short) var(--pc-spring-snappy);
}

.m3chip--selected .m3chip__check {
	width: 1rem;
	opacity: 1;
	transform: scale(1);
}

.m3chip__label {
	white-space: nowrap;
}

/* Reduced motion: fades cap at 100ms, no translation or scale. */
@media (prefers-reduced-motion: reduce) {
	.m3chip,
	.m3chip__check {
		transition-duration: var(--pc-duration-shortest);
	}
	.m3chip:active:not(:disabled),
	.m3chip--selected .m3chip__check {
		transform: none;
	}
}

html[data-reduce-motion] .m3chip,
html[data-reduce-motion] .m3chip__check {
	transition-duration: var(--pc-duration-shortest);
}

html[data-reduce-motion] .m3chip:active:not(:disabled),
html[data-reduce-motion] .m3chip--selected .m3chip__check {
	transform: none;
}
</style>
