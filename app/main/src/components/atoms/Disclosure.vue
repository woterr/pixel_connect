<script setup lang="ts">
// Pixel Connect — animated disclosure.
//
// Height + fade expand for helper copy such as the "Why?" behind
// Not-implemented transports. Works controlled (`open` + `update:open`) or
// uncontrolled (`defaultOpen`). Correct expanded/region ARIA.

import { computed, ref, useId } from 'vue'

const props = withDefaults(
	defineProps<{
		label: string
		open?: boolean | null
		defaultOpen?: boolean
		disabled?: boolean
	}>(),
	{ open: null, defaultOpen: false, disabled: false },
)

const emit = defineEmits<{ 'update:open': [boolean]; toggle: [boolean] }>()

const inner = ref(props.defaultOpen)
const isOpen = computed(() => (props.open !== null ? props.open : inner.value))
const regionId = useId()
const buttonId = useId()

function toggle() {
	if (props.disabled) return
	const next = !isOpen.value
	if (props.open === null) inner.value = next
	emit('update:open', next)
	emit('toggle', next)
}
</script>

<template>
	<div class="m3disc" :class="{ 'm3disc--open': isOpen }">
		<button
			:id="buttonId"
			type="button"
			class="m3disc__button"
			:aria-expanded="isOpen"
			:aria-controls="regionId"
			:disabled="disabled"
			@click="toggle"
		>
			<span class="m3disc__label">{{ label }}</span>
			<span class="m3disc__chevron" aria-hidden="true" />
		</button>
		<div
			:id="regionId"
			class="m3disc__panel"
			role="region"
			:aria-labelledby="buttonId"
			:aria-hidden="!isOpen"
		>
			<div class="m3disc__inner">
				<slot />
			</div>
		</div>
	</div>
</template>

<style scoped>
.m3disc {
	border-radius: var(--pc-radius-inner);
}

.m3disc__button {
	display: flex;
	align-items: center;
	gap: 0.5rem;
	width: 100%;
	min-height: 40px;
	padding: 0.5rem 0.25rem;
	border: 0;
	background: transparent;
	color: rgb(var(--pc-muted));
	font-size: 0.875rem;
	font-weight: 600;
	cursor: pointer;
	transition: color var(--pc-duration-short) var(--pc-ease-emphasized);
}

.m3disc__button:hover:not(:disabled) {
	color: rgb(var(--pc-text));
}

.m3disc__button:focus-visible {
	outline: 2px solid var(--pc-primary);
	outline-offset: 2px;
	border-radius: 0.5rem;
}

.m3disc__button:disabled {
	opacity: 0.4;
	cursor: not-allowed;
}

.m3disc__label {
	flex: 1 1 auto;
	text-align: left;
}

/* CSS chevron: drawn, not an icon-font glyph. */
.m3disc__chevron {
	flex: none;
	width: 0.5625rem;
	height: 0.5625rem;
	border-right: 2px solid currentColor;
	border-bottom: 2px solid currentColor;
	transform: rotate(45deg);
	transition: transform var(--pc-duration-short) var(--pc-ease-emphasized);
}

.m3disc--open .m3disc__chevron {
	transform: rotate(225deg);
}

.m3disc__panel {
	display: grid;
	grid-template-rows: 0fr;
	opacity: 0;
	transition:
		grid-template-rows var(--pc-duration-medium) var(--pc-ease-emphasized),
		opacity var(--pc-duration-medium) var(--pc-ease-emphasized);
}

.m3disc--open .m3disc__panel {
	grid-template-rows: 1fr;
	opacity: 1;
}

.m3disc__inner {
	overflow: hidden;
	min-height: 0;
	font-size: 0.875rem;
	line-height: 1.5rem;
	color: rgb(var(--pc-muted));
}

/* Reduced motion: snap height, quick fade only. */
@media (prefers-reduced-motion: reduce) {
	.m3disc__panel {
		transition: opacity var(--pc-duration-shortest) ease;
	}
	.m3disc__chevron,
	.m3disc__button {
		transition-duration: var(--pc-duration-shortest);
	}
}

html[data-reduce-motion] .m3disc__panel {
	transition: opacity var(--pc-duration-shortest) ease;
}

html[data-reduce-motion] .m3disc__chevron,
html[data-reduce-motion] .m3disc__button {
	transition-duration: var(--pc-duration-shortest);
}
</style>
