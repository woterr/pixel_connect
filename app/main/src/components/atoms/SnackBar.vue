<script setup lang="ts">
// Pixel Connect — Material 3 Expressive snackbar.
//
// Springs up on open; the `action` slot carries the Undo-style action.

import { onUnmounted, watch } from 'vue'

import PcIcon from './PcIcon.vue'

const props = withDefaults(
	defineProps<{
		open: boolean
		/** Auto-dismiss delay in ms; 0 keeps it until dismissed. */
		timeout?: number
	}>(),
	{ timeout: 5000 },
)

const emit = defineEmits<{ close: [] }>()

let timer: ReturnType<typeof setTimeout> | null = null

function clearTimer() {
	if (timer !== null) {
		clearTimeout(timer)
		timer = null
	}
}

watch(
	() => props.open,
	(isOpen) => {
		clearTimer()
		if (isOpen && props.timeout > 0) {
			timer = setTimeout(() => emit('close'), props.timeout)
		}
	},
	{ immediate: true },
)

onUnmounted(clearTimer)

function dismiss() {
	clearTimer()
	emit('close')
}
</script>

<template>
	<div
		v-if="open"
		class="m3snack"
		role="status"
		aria-live="polite"
	>
		<p class="m3snack__msg"><slot /></p>
		<span v-if="$slots.action" class="m3snack__action">
			<slot name="action" />
		</span>
		<button type="button" class="m3snack__dismiss" aria-label="Dismiss" @click="dismiss">
			<PcIcon name="close" :size="16" />
		</button>
	</div>
</template>

<style scoped>
.m3snack {
	display: flex;
	align-items: center;
	gap: 0.5rem;
	min-height: 48px;
	max-width: min(100%, 28rem);
	padding: 0.375rem 0.375rem 0.375rem 1rem;
	border-radius: 0.75rem;
	background-color: rgb(var(--pc-container-highest));
	color: rgb(var(--pc-text));
	box-shadow: 0 12px 32px rgb(0 0 0 / 0.4);
	animation: m3snack-in var(--pc-duration-medium) var(--pc-spring-soft);
}

@keyframes m3snack-in {
	from {
		opacity: 0;
		transform: translateY(16px) scale(0.97);
	}
	to {
		opacity: 1;
		transform: translateY(0) scale(1);
	}
}

.m3snack__msg {
	flex: 1 1 auto;
	min-width: 0;
	margin: 0;
	font-size: 0.875rem;
	line-height: 1.25rem;
}

.m3snack__action {
	display: flex;
	flex: none;
}

.m3snack__dismiss {
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

.m3snack__dismiss:hover {
	background-color: rgb(var(--pc-card-alt));
	color: rgb(var(--pc-text));
}

.m3snack__dismiss:focus-visible {
	outline: 2px solid var(--pc-primary);
	outline-offset: 2px;
}

/* Reduced motion: quick fade, no spring-up. */
@media (prefers-reduced-motion: reduce) {
	.m3snack {
		animation: m3snack-fade var(--pc-duration-shortest) ease;
	}
}

@keyframes m3snack-fade {
	from {
		opacity: 0;
	}
	to {
		opacity: 1;
	}
}

html[data-reduce-motion] .m3snack {
	animation: m3snack-fade var(--pc-duration-shortest) ease;
}
</style>
