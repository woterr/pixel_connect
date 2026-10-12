<script setup lang="ts">
// Pixel Connect — shimmer loading rows.
//
// Placeholder rows with a sweeping sheen (transform-only). Hidden from
// assistive tech apart from the loading label.

import { computed } from 'vue'

const props = withDefaults(
	defineProps<{
		lines?: number
		label?: string
	}>(),
	{ lines: 3, label: 'Loading content' },
)

const rows = computed(() =>
	Array.from({ length: Math.max(1, Math.min(12, Math.round(props.lines))) }, (_, i) => i),
)
</script>

<template>
	<div class="m3skel" role="status" :aria-label="label">
		<span v-for="i in rows" :key="i" class="m3skel__row" aria-hidden="true" />
	</div>
</template>

<style scoped>
.m3skel {
	display: flex;
	flex-direction: column;
	gap: 0.625rem;
}

.m3skel__row {
	position: relative;
	height: 1rem;
	overflow: hidden;
	border-radius: 9999px;
	background-color: rgb(var(--pc-card-alt));
}

.m3skel__row:nth-child(3n + 1) {
	width: 92%;
}

.m3skel__row:nth-child(3n + 2) {
	width: 100%;
}

.m3skel__row:nth-child(3n) {
	width: 68%;
}

/* Sheen sweep: a translated gradient bar, transform-only. */
.m3skel__row::after {
	content: '';
	position: absolute;
	top: 0;
	bottom: 0;
	left: 0;
	width: 40%;
	border-radius: inherit;
	background: linear-gradient(
		90deg,
		transparent,
		rgb(var(--pc-text) / 0.08),
		transparent
	);
	transform: translateX(-100%);
	animation: m3skel-sweep var(--pc-duration-long) var(--pc-ease-decelerate) infinite;
}

@keyframes m3skel-sweep {
	to {
		transform: translateX(280%);
	}
}

/* Reduced motion: static rows, no sweep. */
@media (prefers-reduced-motion: reduce) {
	.m3skel__row::after {
		animation: none;
		opacity: 0;
	}
}

html[data-reduce-motion] .m3skel__row::after {
	animation: none;
	opacity: 0;
}
</style>
