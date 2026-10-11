<script setup lang="ts">
// Pixel Connect — brand mark.
//
// The supplied mark is a dark gradient (#808080 -> #000000 -> #808080) drawn
// for a light surface. This application renders on a near-black canvas, where
// that artwork has almost no contrast, so the mark is carried on a neutral
// light plate instead of being recoloured.
//
// The artwork is never redrawn, simplified or approximated: the file from
// `data/branding/` is used byte-for-byte, and `object-fit: contain` keeps its
// aspect ratio so it is never stretched.
//
// Clearspace: the mark is authored on a 108-unit box with an 11.5-unit ribbon,
// and its ink spans x 20..88, so the file already carries 20/108 = 1.74x
// ribbon thickness of margin on each side. The plate padding below adds a
// further ~0.75x, which clears the 2x minimum without crowding the mark.

import { computed } from 'vue'

import markUrl from '../../assets/brand/pixel-connect-mark.svg'

const props = withDefaults(
	defineProps<{
		/** Rendered edge length of the mark artwork, in px. Use >= 64 for the gradient. */
		size?: number
		/** Show the surrounding plate. Off when the surface behind is already light. */
		plate?: boolean
	}>(),
	{ size: 64, plate: true },
)

/** Padding on each side, as a fraction of the artwork size. */
const PAD_RATIO = 0.08
const plateSize = computed(() => Math.round(props.size * (1 + PAD_RATIO * 2)))
</script>

<template>
	<span
		class="pc-brand__plate"
		:style="{ width: `${plateSize}px`, height: `${plateSize}px` }"
	>
		<img
			class="pc-brand__mark"
			:src="markUrl"
			:style="{ width: `${props.size}px`, height: `${props.size}px` }"
			alt="Pixel Connect"
			width="108"
			height="108"
			draggable="false"
		/>
	</span>
</template>
