<script setup lang="ts">
// Pixel Connect — Material 3 Expressive list row.
//
// >= 64px tall with a leading icon, title, supporting text, and a trailing
// control slot. Renders as a button when `interactive`.

import PcIcon from './PcIcon.vue'

withDefaults(
	defineProps<{
		title: string
		supportingText?: string
		leadingIcon?: string
		interactive?: boolean
		disabled?: boolean
		selected?: boolean
	}>(),
	{
		supportingText: undefined,
		leadingIcon: undefined,
		interactive: false,
		disabled: false,
		selected: false,
	},
)

defineEmits<{ click: [MouseEvent] }>()
</script>

<template>
	<component
		:is="interactive ? 'button' : 'div'"
		:type="interactive ? 'button' : undefined"
		class="m3row"
		:class="{ 'm3row--selected': selected }"
		:disabled="interactive ? disabled : undefined"
		@click="$emit('click', $event)"
	>
		<span v-if="leadingIcon || $slots.leading" class="m3row__leading" aria-hidden="true">
			<slot name="leading">
				<PcIcon v-if="leadingIcon" :name="leadingIcon" :size="24" />
			</slot>
		</span>
		<span class="m3row__body">
			<span class="m3row__title"><slot name="title">{{ title }}</slot></span>
			<span v-if="supportingText || $slots.supporting" class="m3row__supporting">
				<slot name="supporting">{{ supportingText }}</slot>
			</span>
			<span class="m3row__extra"><slot /></span>
		</span>
		<span v-if="$slots.trailing" class="m3row__trailing">
			<slot name="trailing" />
		</span>
	</component>
</template>

<style scoped>
.m3row {
	display: flex;
	align-items: center;
	gap: 1rem;
	width: 100%;
	min-height: 64px;
	padding: 0.75rem 1rem;
	border: 0;
	border-radius: var(--pc-radius-inner);
	background-color: transparent;
	color: rgb(var(--pc-text));
	text-align: left;
	font: inherit;
	transition: background-color var(--pc-duration-short) var(--pc-ease-emphasized);
}

button.m3row {
	cursor: pointer;
}

button.m3row:hover:not(:disabled) {
	background-color: rgb(var(--pc-card-alt));
}

button.m3row:focus-visible {
	outline: 2px solid var(--pc-primary);
	outline-offset: 2px;
}

button.m3row:disabled {
	opacity: 0.4;
	cursor: not-allowed;
}

.m3row--selected {
	background-color: rgb(var(--pc-card-alt));
}

.m3row__leading {
	display: flex;
	align-items: center;
	justify-content: center;
	flex: none;
	width: 40px;
	height: 40px;
	border-radius: 9999px;
	color: rgb(var(--pc-muted));
}

.m3row__body {
	display: flex;
	flex-direction: column;
	gap: 0.125rem;
	min-width: 0;
	flex: 1 1 auto;
}

.m3row__title {
	font-size: 1rem;
	line-height: 1.5rem;
	font-weight: 500;
	overflow: hidden;
	text-overflow: ellipsis;
	white-space: nowrap;
}

.m3row__supporting {
	font-size: 0.875rem;
	line-height: 1.25rem;
	color: rgb(var(--pc-muted));
	overflow: hidden;
	text-overflow: ellipsis;
	white-space: nowrap;
}

.m3row__extra:empty {
	display: none;
}

.m3row__trailing {
	display: flex;
	align-items: center;
	gap: 0.5rem;
	flex: none;
}

/* Reduced motion: fades cap at 100ms. */
@media (prefers-reduced-motion: reduce) {
	.m3row {
		transition-duration: var(--pc-duration-shortest);
	}
}

html[data-reduce-motion] .m3row {
	transition-duration: var(--pc-duration-shortest);
}
</style>
