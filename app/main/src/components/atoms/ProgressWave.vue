<script setup lang="ts">
// Pixel Connect — wavy transfer progress.
//
// The signature element from the supplied designs: a sine-wave rule that fills
// left-to-right as a transfer progresses. The wave geometry is generated from
// the measured path length so the fill always lines up with the track.

import { computed, onMounted, ref } from 'vue'

const props = withDefaults(
	defineProps<{
		/** 0..1 */
		value: number
		/** Number of wave crests across the full width. */
		waves?: number
		height?: number
	}>(),
	{ waves: 7, height: 26 },
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

onMounted(() => {
	if (pathRef.value) {
		length.value = pathRef.value.getTotalLength() || 1000
	}
})
</script>

<template>
	<svg
		class="pc-wave"
		:viewBox="`0 0 ${W} ${height}`"
		preserveAspectRatio="none"
		role="progressbar"
		:aria-valuenow="Math.round(clamped * 100)"
		aria-valuemin="0"
		aria-valuemax="100"
		aria-label="Transfer progress"
	>
		<path class="pc-wave__track" :d="d" />
		<path
			ref="pathRef"
			class="pc-wave__fill"
			:d="d"
			:stroke-dasharray="dash"
			:stroke-dashoffset="offset"
		/>
	</svg>
</template>