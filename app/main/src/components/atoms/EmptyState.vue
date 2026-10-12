<script setup lang="ts">
// Pixel Connect — empty state.
//
// Brand mark slot (BrandMark by default), title, body, and an action slot.

import BrandMark from './BrandMark.vue'

withDefaults(
	defineProps<{
		title: string
		body?: string
		markSize?: number
	}>(),
	{ body: undefined, markSize: 64 },
)
</script>

<template>
	<div class="m3empty" role="status">
		<span class="m3empty__mark">
			<slot name="mark">
				<BrandMark :size="markSize" />
			</slot>
		</span>
		<h2 class="m3empty__title">{{ title }}</h2>
		<p v-if="body || $slots.default" class="m3empty__body">
			<slot>{{ body }}</slot>
		</p>
		<div v-if="$slots.action" class="m3empty__action">
			<slot name="action" />
		</div>
	</div>
</template>

<style scoped>
.m3empty {
	display: flex;
	flex-direction: column;
	align-items: center;
	gap: 0.75rem;
	max-width: 26rem;
	margin-inline: auto;
	padding: 2rem 1.5rem;
	text-align: center;
}

.m3empty__mark {
	display: flex;
	justify-content: center;
}

.m3empty__title {
	margin: 0.5rem 0 0;
	font-size: 1.25rem;
	line-height: 1.75rem;
	font-weight: 600;
	color: rgb(var(--pc-text));
}

.m3empty__body {
	margin: 0;
	font-size: 0.9375rem;
	line-height: 1.5rem;
	color: rgb(var(--pc-muted));
}

.m3empty__action {
	display: flex;
	justify-content: center;
	margin-top: 0.5rem;
}
</style>
