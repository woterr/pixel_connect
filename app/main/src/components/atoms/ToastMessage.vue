<script setup lang="ts">
// Pixel Connect — toast notifications.

import PcIcon from './PcIcon.vue'

defineProps<{ id: number; kind: 'info' | 'success' | 'error'; message: string }>()
const emit = defineEmits<{ dismiss: [number] }>()
</script>

<template>
	<div
		class="pc-card pointer-events-auto flex items-start gap-3 px-4 py-3 text-sm shadow-2xl"
		role="status"
		:aria-live="kind === 'error' ? 'assertive' : 'polite'"
	>
		<PcIcon
			:name="kind === 'error' ? 'alert' : kind === 'success' ? 'check' : 'info'"
			:size="18"
			class="mt-0.5"
			:class="{
				'text-danger': kind === 'error',
				'text-online': kind === 'success',
				'text-muted': kind === 'info',
			}"
		/>
		<p class="flex-1 leading-snug">{{ message }}</p>
		<button
			type="button"
			class="text-faint transition-colors hover:text-ink"
			aria-label="Dismiss"
			@click="emit('dismiss', id)"
		>
			<PcIcon name="close" :size="16" />
		</button>
	</div>
</template>