<template>
	<!-- Pixel Connect — legacy toggle alias. Re-exported from Switch.vue so
		existing imports keep working with identical props/events. -->
	<Switch
		:model-value="modelValue"
		:label="label"
		:disabled="disabled"
		@update:model-value="$emit('update:modelValue', $event)"
	/>
</template>

<script setup lang="ts">
import Switch from './Switch.vue'

defineProps<{
	modelValue: boolean
	label?: string
	disabled?: boolean
}>()

defineEmits<{ 'update:modelValue': [boolean] }>()
</script>
