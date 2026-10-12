<script setup lang="ts">
// Pixel Connect — logo sprite (single instance).
//
// One inline `<symbol>` holding the tile, gradients and weave mask, referenced
// everywhere else with `<use href="#pc-logo">`, so the gradient and mask ids
// exist exactly once no matter how many lockups are on screen.
//
// Geometry is the supplied artwork, unaltered: rx = 26/108 tile, tile gradient
// (0,0)->(0.6,1) #FFFFFF->#E8E8E8, ink gradient
// #808080->#303030->#000000->#303030->#808080, 5-unit crossover gap.
</script>

<template>
	<svg width="0" height="0" style="position: absolute" aria-hidden="true">
		<symbol id="pc-logo" viewBox="0 0 108 108">
			<defs>
				<linearGradient id="pc-tile" x1="0" y1="0" x2="0.6" y2="1">
					<stop offset="0" stop-color="#FFFFFF" />
					<stop offset="1" stop-color="#E8E8E8" />
				</linearGradient>
				<linearGradient
					id="pc-ink"
					gradientUnits="userSpaceOnUse"
					x1="13"
					y1="0"
					x2="95"
					y2="0"
				>
					<stop offset="0" stop-color="#808080" />
					<stop offset="0.16" stop-color="#303030" />
					<stop offset="0.5" stop-color="#000000" />
					<stop offset="0.84" stop-color="#303030" />
					<stop offset="1" stop-color="#808080" />
				</linearGradient>
				<mask
					id="pc-weave"
					maskUnits="userSpaceOnUse"
					x="0"
					y="0"
					width="108"
					height="108"
				>
					<rect width="108" height="108" fill="#fff" />
					<path
						d="M36.5 69C47.88 69 46.5 61.5 54 54C61.5 46.5 60.12 39 71.5 39"
						fill="none"
						stroke="#000"
						stroke-width="21.5"
					/>
				</mask>
			</defs>
			<rect width="108" height="108" rx="26" fill="url(#pc-tile)" class="pc-logo__tile-shape" />
			<g
				transform="translate(54 54) scale(0.92) translate(-54 -54)"
				fill="none"
				stroke="url(#pc-ink)"
				stroke-width="11.5"
				stroke-linejoin="round"
			>
				<path
					mask="url(#pc-weave)"
					d="M87.61 48.14A17.5 15 0 0 1 71.5 69C60.12 69 61.5 61.5 54 54C46.5 46.5 47.88 39 36.5 39A17.5 15 0 0 0 20.39 59.86"
				/>
				<path
					d="M29.66 67.81A17.5 15 0 0 0 36.5 69C47.88 69 46.5 61.5 54 54C61.5 46.5 60.12 39 71.5 39A17.5 15 0 0 1 78.34 40.19"
				/>
			</g>
		</symbol>
		<!-- Flat single-colour mark for the tray: same paths, currentColor, no tile. -->
		<symbol id="pc-mark-flat" viewBox="9 26 90 56">
			<g
				fill="none"
				stroke="currentColor"
				stroke-width="11.5"
				stroke-linejoin="round"
			>
				<path
					d="M87.61 48.14A17.5 15 0 0 1 71.5 69C60.12 69 61.5 61.5 54 54C46.5 46.5 47.88 39 36.5 39A17.5 15 0 0 0 20.39 59.86"
				/>
				<path
					d="M29.66 67.81A17.5 15 0 0 0 36.5 69C47.88 69 46.5 61.5 54 54C61.5 46.5 60.12 39 71.5 39A17.5 15 0 0 1 78.34 40.19"
				/>
			</g>
		</symbol>
	</svg>
</template>

<style>
/* Hairline separation on light themes only; on dark the white tile separates itself. */
html[data-theme='simple-light'] .pc-logo__tile-shape,
html[data-theme='white-light'] .pc-logo__tile-shape,
html[data-theme='light'] .pc-logo__tile-shape,
html[data-theme='contrast-light'] .pc-logo__tile-shape {
	stroke: rgba(0, 0, 0, 0.08);
	stroke-width: 1;
}
</style>
