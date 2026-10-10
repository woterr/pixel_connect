<script setup lang="ts">
// Pixel Connect — toggle switch.
//
// A pill-shaped switch with an animated thumb, matching the settings screens in
// the supplied designs.

defineProps<{
	modelValue: boolean
	label?: string
	disabled?: boolean
}>()

const emit = defineEmits<{ 'update:modelValue': [boolean] }>()
</script>

<template>
	<button
		type="button"
		role="switch"
		class="pc-switch"
		:aria-checked="modelValue"
		:aria-label="label"
		:disabled="disabled"
		@click="emit('update:modelValue', !modelValue)"
	>
		<span class="pc-switch__thumb" />
	</button>
</template>