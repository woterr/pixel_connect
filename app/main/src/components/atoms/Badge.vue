<script setup lang="ts">
// Pixel Connect — animated count badge.
//
// Rolls/pops on count change via a keyed re-mount.

import { computed } from 'vue'

const props = withDefaults(
	defineProps<{
		count: number
		max?: number
		showZero?: boolean
		label?: string
	}>(),
	{ max: 99, showZero: false, label: 'notifications' },
)

const display = computed(() => (props.count > props.max ? `${props.max}+` : `${props.count}`))
const visible = computed(() => props.showZero || props.count > 0)
</script>

<template>
	<span
		v-if="visible"
		:key="display"
		class="m3badge"
		role="status"
		:aria-label="`${display} ${label}`"
	>
		{{ display }}
	</span>
</template>

<style scoped>
.m3badge {
	display: inline-flex;
	align-items: center;
	justify-content: center;
	min-width: 1.375rem;
	height: 1.375rem;
	padding: 0 0.375rem;
	border-radius: 9999px;
	background-color: rgb(var(--pc-danger));
	color: #ffffff;
	font-size: 0.75rem;
	font-weight: 700;
	line-height: 1;
	font-variant-numeric: tabular-nums;
	animation: m3badge-pop var(--pc-duration-short) var(--pc-spring-snappy);
}

@keyframes m3badge-pop {
	from {
		opacity: 0;
		transform: scale(0.4);
	}
	to {
		opacity: 1;
		transform: scale(1);
	}
}

/* Reduced motion: quick fade, no pop. */
@media (prefers-reduced-motion: reduce) {
	.m3badge {
		animation-duration: var(--pc-duration-shortest);
		animation-name: m3badge-fade;
	}
}

@keyframes m3badge-fade {
	from {
		opacity: 0;
	}
	to {
		opacity: 1;
	}
}

html[data-reduce-motion] .m3badge {
	animation-duration: var(--pc-duration-shortest);
	animation-name: m3badge-fade;
}
</style>
