<script setup lang="ts">
// Pixel Connect — application icon.
//
// This is the *full* brand icon: the rounded plate with the mark inside it,
// supplied as-is in `data/branding/`. It is shown only where an app-icon
// context is called for (About, Help).
//
// The icon already carries its own rounded plate, so it is deliberately NOT
// placed inside another rounded container — no `.pc-card`, no border-radius,
// no framing chrome. It is presented bare on the page surface.

import { computed } from 'vue'

import lightUrl from '../../assets/brand/pixel-connect-icon-light.svg'
import darkUrl from '../../assets/brand/pixel-connect-icon-dark.svg'

const props = withDefaults(
	defineProps<{
		size?: number
		/**
		 * Which supplied variant to use.
		 *
		 * `light` is the white plate with the dark mark; `dark` is the black
		 * plate with the white mark. The canvas here is near-black, so `light`
		 * is the legible default and `dark` is for a light surface.
		 */
		variant?: 'light' | 'dark'
	}>(),
	{ size: 96, variant: 'light' },
)

const src = computed(() => (props.variant === 'dark' ? darkUrl : lightUrl))
</script>

<template>
	<img
		:src="src"
		:width="props.size"
		:height="props.size"
		:style="{ width: `${props.size}px`, height: `${props.size}px` }"
		alt="Pixel Connect application icon"
		draggable="false"
		decoding="async"
		class="block flex-none"
	/>
</template>
