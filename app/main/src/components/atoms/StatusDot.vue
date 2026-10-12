<script setup lang="ts">
// Pixel Connect — word + colour state dot.
//
// State is never colour-alone: every dot carries its word. While searching
// a soft ring pulses (scale + fade only).

import { computed } from 'vue'

type State = 'online' | 'searching' | 'offline' | 'error'

const props = withDefaults(
	defineProps<{
		state: State
		/** Override the displayed word (defaults to the state word). */
		label?: string
	}>(),
	{ label: undefined },
)

const WORDS: Record<State, string> = {
	online: 'Online',
	searching: 'Searching',
	offline: 'Offline',
	error: 'Error',
}

const word = computed(() => props.label ?? WORDS[props.state])
</script>

<template>
	<span class="m3dot" :data-state="state" role="status">
		<span class="m3dot__dot" aria-hidden="true">
			<span v-if="state === 'searching'" class="m3dot__ring" aria-hidden="true" />
		</span>
		<span class="m3dot__word">{{ word }}</span>
	</span>
</template>

<style scoped>
.m3dot {
	display: inline-flex;
	align-items: center;
	gap: 0.5rem;
	font-size: 0.875rem;
	font-weight: 600;
	line-height: 1.25rem;
	white-space: nowrap;
}

.m3dot__dot {
	position: relative;
	width: 0.625rem;
	height: 0.625rem;
	flex: none;
	border-radius: 9999px;
	background-color: rgb(var(--pc-faint));
}

.m3dot[data-state='online'] {
	color: rgb(var(--pc-online));
}

.m3dot[data-state='online'] .m3dot__dot {
	background-color: rgb(var(--pc-online));
}

.m3dot[data-state='searching'] {
	color: rgb(var(--pc-warn));
}

.m3dot[data-state='searching'] .m3dot__dot {
	background-color: rgb(var(--pc-warn));
}

.m3dot[data-state='error'] {
	color: rgb(var(--pc-danger));
}

.m3dot[data-state='error'] .m3dot__dot {
	background-color: rgb(var(--pc-danger));
}

.m3dot[data-state='offline'] {
	color: rgb(var(--pc-muted));
}

.m3dot__ring {
	position: absolute;
	inset: 0;
	border-radius: 9999px;
	border: 2px solid rgb(var(--pc-warn));
	animation: m3dot-pulse var(--pc-duration-long) var(--pc-ease-decelerate) infinite;
}

@keyframes m3dot-pulse {
	from {
		opacity: 0.7;
		transform: scale(1);
	}
	to {
		opacity: 0;
		transform: scale(2.1);
	}
}

.m3dot__word {
	letter-spacing: 0.01em;
}

/* Reduced motion: steady dot, no pulse. */
@media (prefers-reduced-motion: reduce) {
	.m3dot__ring {
		animation: none;
		opacity: 0.5;
	}
}

html[data-reduce-motion] .m3dot__ring {
	animation: none;
	opacity: 0.5;
}
</style>
