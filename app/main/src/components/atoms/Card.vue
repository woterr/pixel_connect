<script setup lang="ts">
// Pixel Connect — Material 3 Expressive tonal card.
//
// 24px radius; hoverable cards raise the tone.

withDefaults(
	defineProps<{
		tag?: 'div' | 'section' | 'article'
		hoverable?: boolean
		padded?: boolean
	}>(),
	{ tag: 'div', hoverable: false, padded: true },
)
</script>

<template>
	<component :is="tag" class="m3card" :class="{ 'm3card--hover': hoverable, 'm3card--pad': padded }">
		<slot />
	</component>
</template>

<style scoped>
.m3card {
	background-color: rgb(var(--pc-card));
	color: rgb(var(--pc-text));
	border-radius: var(--pc-radius-card);
	transition:
		background-color var(--pc-duration-short) var(--pc-ease-emphasized),
		transform var(--pc-duration-short) var(--pc-ease-emphasized);
}

.m3card--pad {
	padding: 1.25rem;
}

.m3card--hover:hover {
	background-color: rgb(var(--pc-card-alt));
}

.m3card--hover:active {
	transform: scale(0.995);
}

/* Reduced motion: fades cap at 100ms, no scale. */
@media (prefers-reduced-motion: reduce) {
	.m3card {
		transition-duration: var(--pc-duration-shortest);
	}
	.m3card--hover:active {
		transform: none;
	}
}

html[data-reduce-motion] .m3card {
	transition-duration: var(--pc-duration-shortest);
}

html[data-reduce-motion] .m3card--hover:active {
	transform: none;
}
</style>
