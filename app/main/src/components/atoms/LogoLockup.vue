<script setup lang="ts">
// Pixel Connect — logo lockup: white tile (via the shared `#pc-logo` sprite)
// plus the Google Sans Flex wordmark at the maximum weight (1000).
//
// The tile is identical in every theme (white gradient per the brand spec).
// The wordmark uses the theme's normal text colour.
</script>

<template>
	<span class="pc-lockup">
		<svg
			class="pc-lockup__tile"
			width="40"
			height="40"
			viewBox="0 0 108 108"
			role="img"
			aria-label="Pixel Connect logo"
		>
			<!-- xlink attr is for older WebKit (incl. some WebKitGTK) which
				ignores plain href; modern browsers use href. One element, so
				the tile paints exactly once. -->
			<use href="#pc-logo" xmlns:xlink="http://www.w3.org/1999/xlink" xlink:href="#pc-logo" />
		</svg>
		<span class="pc-lockup__word">Pixel&nbsp;Connect</span>
	</span>
</template>

<style scoped>
.pc-lockup {
	display: inline-flex;
	align-items: center;
	gap: 0.75rem;
	min-width: 0;
}
.pc-lockup__tile {
	flex: none;
	display: block;
}
.pc-lockup__word {
	font-family: var(--pc-font-sans);
	font-weight: 1000;
	font-style: normal;
	font-synthesis: none;
	letter-spacing: -0.01em;
	font-size: 1.125rem;
	line-height: 1.2;
	color: rgb(var(--pc-text));
	white-space: nowrap;
	overflow: hidden;
	text-overflow: ellipsis;
}
</style>
