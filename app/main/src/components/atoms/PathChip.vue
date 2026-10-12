<script setup lang="ts">
// Pixel Connect — monospace path with a copy button.
//
// Copies the exact `path` prop via the clipboard API; feedback is a check
// swap plus a polite live message. Reuses PcIcon only.

import { onUnmounted, ref } from 'vue'

import PcIcon from './PcIcon.vue'

const props = withDefaults(
	defineProps<{
		path: string
		label?: string
	}>(),
	{ label: 'Path' },
)

const copied = ref(false)
const failed = ref(false)
let timer: ReturnType<typeof setTimeout> | null = null

function clearTimer() {
	if (timer !== null) {
		clearTimeout(timer)
		timer = null
	}
}

function flash(ok: boolean) {
	clearTimer()
	copied.value = ok
	failed.value = !ok
	timer = setTimeout(() => {
		copied.value = false
		failed.value = false
	}, 1500)
}

async function copy() {
	try {
		if (!navigator.clipboard) throw new Error('clipboard unavailable')
		await navigator.clipboard.writeText(props.path)
		flash(true)
	} catch {
		flash(false)
	}
}

onUnmounted(clearTimer)
</script>

<template>
	<div class="m3path">
		<code class="m3path__text" :aria-label="label">{{ path }}</code>
		<button
			type="button"
			class="m3path__copy"
			:aria-label="copied ? 'Copied' : `Copy ${label.toLowerCase()}`"
			:title="failed ? 'Copy failed' : `Copy ${label.toLowerCase()}`"
			@click="copy"
		>
			<PcIcon v-if="!copied" name="copy" :size="16" />
			<PcIcon v-else name="check" :size="16" class="m3path__copied" />
		</button>
		<span class="m3path__live" aria-live="polite">
			{{ copied ? 'Copied to clipboard' : failed ? 'Copy failed' : '' }}
		</span>
	</div>
</template>

<style scoped>
.m3path {
	display: flex;
	align-items: center;
	gap: 0.5rem;
	min-width: 0;
	padding: 0.25rem 0.25rem 0.25rem 0.75rem;
	border-radius: var(--pc-radius-inner);
	border: 1px solid rgb(var(--pc-line));
	background-color: rgb(var(--pc-card-alt));
}

.m3path__text {
	flex: 1 1 auto;
	min-width: 0;
	overflow: hidden;
	text-overflow: ellipsis;
	white-space: nowrap;
	font-family: var(--pc-font-mono);
	font-size: 0.875rem;
	line-height: 1.25rem;
	color: rgb(var(--pc-text));
}

.m3path__copy {
	display: flex;
	align-items: center;
	justify-content: center;
	flex: none;
	width: 40px;
	height: 40px;
	border-radius: 0.75rem;
	color: rgb(var(--pc-muted));
	cursor: pointer;
	transition:
		transform var(--pc-duration-shortest) var(--pc-ease-emphasized),
		background-color var(--pc-duration-shortest) var(--pc-ease-emphasized),
		color var(--pc-duration-shortest) var(--pc-ease-emphasized);
}

.m3path__copy:hover {
	background-color: rgb(var(--pc-card));
	color: rgb(var(--pc-text));
}

.m3path__copy:focus-visible {
	outline: 2px solid var(--pc-primary);
	outline-offset: 2px;
}

.m3path__copy:active {
	transform: scale(0.97);
}

.m3path__copied {
	color: rgb(var(--pc-online));
}

.m3path__live {
	position: absolute;
	width: 1px;
	height: 1px;
	overflow: hidden;
	clip: rect(0 0 0 0);
	white-space: nowrap;
}

/* Reduced motion: fades cap at 100ms, no scale. */
@media (prefers-reduced-motion: reduce) {
	.m3path__copy {
		transition-duration: var(--pc-duration-shortest);
	}
	.m3path__copy:active {
		transform: none;
	}
}

html[data-reduce-motion] .m3path__copy {
	transition-duration: var(--pc-duration-shortest);
}

html[data-reduce-motion] .m3path__copy:active {
	transform: none;
}
</style>
