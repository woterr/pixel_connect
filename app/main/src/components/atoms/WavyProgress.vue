<script setup lang="ts">
// Pixel Connect — expressive wavy transfer progress.
//
// Same geometry contract as ProgressWave (`value` 0..1, `waves`, `height`)
// plus a connection `state`:
//   offline   flat line (amplitude 0)
//   connected gentle sway at reduced amplitude
//   transfer  full wave, drifting while bytes move
//   complete  determinate full, amplitude easing back to 0
// `speed` and `eta` slots carry the determinate readout. Motion is
// transform/opacity only (scaleY for amplitude, translateX for drift).

import { computed, onMounted, ref } from 'vue'

const props = withDefaults(
	defineProps<{
		/** 0..1 */
		value: number
		/** Number of wave crests across the full width. */
		waves?: number
		height?: number
		state?: 'offline' | 'connected' | 'transfer' | 'complete'
	}>(),
	{ waves: 7, height: 26, state: 'transfer' },
)

const pathRef = ref<SVGPathElement | null>(null)
const length = ref(1000)

const W = 1000
const AMP = 7

const d = computed(() => {
	const steps = 120
	const period = W / props.waves
	let out = `M0 ${props.height / 2}`
	for (let i = 1; i <= steps; i++) {
		const x = (i / steps) * W
		const y = props.height / 2 + Math.sin((x / period) * Math.PI * 2) * AMP
		out += ` L${x.toFixed(2)} ${y.toFixed(2)}`
	}
	return out
})

const clamped = computed(() => Math.max(0, Math.min(1, props.value)))
const dash = computed(() => `${length.value}`)
const offset = computed(() => length.value * (1 - clamped.value))

/** Amplitude as a vertical scale: 0 reads as a flat rule. */
const ampScale = computed(() => {
	switch (props.state) {
		case 'offline':
			return 0
		case 'connected':
			return 0.45
		case 'complete':
			return 0
		default:
			return 1
	}
})

onMounted(() => {
	try {
		if (pathRef.value) length.value = pathRef.value.getTotalLength() || 1000
	} catch {
		length.value = 1000
	}
})
</script>

<template>
	<div class="wavy">
		<svg
			class="wavy__svg"
			:viewBox="`0 0 ${W} ${height}`"
			preserveAspectRatio="none"
			role="progressbar"
			:aria-valuenow="Math.round(clamped * 100)"
			aria-valuemin="0"
			aria-valuemax="100"
			aria-label="Transfer progress"
		>
			<path class="wavy__track" :d="d" />
			<g class="wavy__drift" :data-state="state">
				<g class="wavy__amp" :style="{ '--wavy-amp': ampScale }">
					<path
						ref="pathRef"
						class="wavy__fill"
						:data-state="state"
						:d="d"
						:stroke-dasharray="dash"
						:stroke-dashoffset="offset"
					/>
				</g>
			</g>
		</svg>
		<div v-if="$slots.speed || $slots.eta" class="wavy__meta">
			<span class="wavy__speed"><slot name="speed" /></span>
			<span class="wavy__eta"><slot name="eta" /></span>
		</div>
	</div>
</template>

<style scoped>
.wavy {
	display: block;
	width: 100%;
}

.wavy__svg {
	display: block;
	width: 100%;
	height: 26px;
	overflow: hidden;
}

.wavy__track {
	stroke: rgb(var(--pc-line));
	stroke-width: 2;
	fill: none;
	stroke-linecap: round;
}

.wavy__amp {
	transform: scaleY(var(--wavy-amp, 1));
	transform-box: fill-box;
	transform-origin: center;
	transition: transform var(--pc-duration-medium) var(--pc-ease-emphasized);
}

.wavy__fill {
	stroke: rgb(var(--pc-text));
	stroke-width: 2.5;
	fill: none;
	stroke-linecap: round;
	transition: stroke-dashoffset var(--pc-duration-medium) var(--pc-ease-emphasized);
}

.wavy__fill[data-state='complete'] {
	stroke: rgb(var(--pc-online));
}

.wavy__fill[data-state='offline'] {
	stroke: rgb(var(--pc-faint));
}

.wavy__drift[data-state='transfer'] {
	animation: wavy-drift var(--pc-duration-medium) ease-in-out infinite alternate;
}

.wavy__drift[data-state='connected'] {
	animation: wavy-sway var(--pc-duration-long) ease-in-out infinite alternate;
}

@keyframes wavy-drift {
	from {
		transform: translateX(-8px);
	}
	to {
		transform: translateX(8px);
	}
}

@keyframes wavy-sway {
	from {
		transform: translateX(-3px);
	}
	to {
		transform: translateX(3px);
	}
}

.wavy__meta {
	display: flex;
	align-items: baseline;
	justify-content: space-between;
	gap: 1rem;
	margin-top: 0.25rem;
	font-size: 0.875rem;
	line-height: 1.25rem;
	color: rgb(var(--pc-muted));
	font-variant-numeric: tabular-nums;
}

.wavy__speed:empty,
.wavy__eta:empty {
	display: none;
}

.wavy__eta {
	margin-left: auto;
}

/* Reduced motion: no drift, quick fades only. */
@media (prefers-reduced-motion: reduce) {
	.wavy__drift[data-state='transfer'],
	.wavy__drift[data-state='connected'] {
		animation: none;
	}
	.wavy__amp,
	.wavy__fill {
		transition-duration: var(--pc-duration-shortest);
	}
}

html[data-reduce-motion] .wavy__drift[data-state='transfer'],
html[data-reduce-motion] .wavy__drift[data-state='connected'] {
	animation: none;
}

html[data-reduce-motion] .wavy__amp,
html[data-reduce-motion] .wavy__fill {
	transition-duration: var(--pc-duration-shortest);
}
</style>
