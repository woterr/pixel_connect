<script setup lang="ts">
// Pixel Connect — Material 3 Expressive filled text field.
//
// Floating label, focus indicator, and error state. Body text stays at 16px.

import { computed, useId } from 'vue'

const props = withDefaults(
	defineProps<{
		modelValue: string
		label: string
		hint?: string
		error?: string
		type?: string
		disabled?: boolean
		required?: boolean
		autocomplete?: string
	}>(),
	{
		hint: undefined,
		error: undefined,
		type: 'text',
		disabled: false,
		required: false,
		autocomplete: undefined,
	},
)

const emit = defineEmits<{ 'update:modelValue': [string] }>()

const id = useId()
const describedBy = computed(() => {
	const parts: string[] = []
	if (props.hint) parts.push(`${id}-hint`)
	if (props.error) parts.push(`${id}-error`)
	return parts.length ? parts.join(' ') : undefined
})

function onInput(e: Event) {
	emit('update:modelValue', (e.target as HTMLInputElement).value)
}
</script>

<template>
	<div class="m3field" :class="{ 'm3field--error': !!error, 'm3field--disabled': disabled }">
		<div class="m3field__box">
			<input
				:id="id"
				class="m3field__input"
				:value="modelValue"
				:type="type"
				placeholder=" "
				:disabled="disabled"
				:required="required"
				:autocomplete="autocomplete"
				:aria-invalid="!!error"
				:aria-describedby="describedBy"
				@input="onInput"
			/>
			<label class="m3field__label" :for="id">
				{{ label }}<span v-if="required" aria-hidden="true"> *</span>
			</label>
			<span class="m3field__indicator" aria-hidden="true" />
		</div>
		<p v-if="error" :id="`${id}-error`" class="m3field__msg m3field__msg--error" role="alert">
			{{ error }}
		</p>
		<p v-else-if="hint" :id="`${id}-hint`" class="m3field__msg">
			{{ hint }}
		</p>
	</div>
</template>

<style scoped>
.m3field {
	display: flex;
	flex-direction: column;
	gap: 0.25rem;
	min-width: 0;
}

.m3field__box {
	position: relative;
	background-color: rgb(var(--pc-card-alt));
	border-radius: var(--pc-radius-field) var(--pc-radius-field) 0 0;
	transition: background-color var(--pc-duration-short) var(--pc-ease-emphasized);
}

.m3field__box:hover:not(:has(.m3field__input:disabled)) {
	filter: brightness(1.06);
}

.m3field__input {
	width: 100%;
	min-height: 56px;
	padding: 1.375rem 1rem 0.5rem;
	border: 0;
	border-radius: inherit;
	background: transparent;
	color: rgb(var(--pc-text));
	font-size: 1rem;
	line-height: 1.5rem;
	caret-color: var(--pc-primary);
}

.m3field__input::placeholder {
	color: transparent;
}

.m3field__input:focus {
	outline: none;
}

.m3field__label {
	position: absolute;
	top: 1.0625rem;
	left: 1rem;
	color: rgb(var(--pc-muted));
	font-size: 1rem;
	line-height: 1.5rem;
	pointer-events: none;
	transition:
		transform var(--pc-duration-short) var(--pc-ease-emphasized),
		color var(--pc-duration-short) var(--pc-ease-emphasized),
		font-size var(--pc-duration-short) var(--pc-ease-emphasized);
}

/* Float when focused or filled. */
.m3field__input:focus + .m3field__label,
.m3field__input:not(:placeholder-shown) + .m3field__label {
	transform: translateY(-0.75rem);
	font-size: 0.75rem;
	line-height: 1rem;
}

.m3field__input:focus + .m3field__label {
	color: var(--pc-primary);
}

.m3field__indicator {
	position: absolute;
	right: 0;
	bottom: 0;
	left: 0;
	height: 1px;
	background-color: rgb(var(--pc-outline));
	transition: background-color var(--pc-duration-short) var(--pc-ease-emphasized);
}

/* Focus indicator: 2px accent underline. */
.m3field__box:focus-within .m3field__indicator {
	height: 2px;
	background-color: var(--pc-primary);
}

.m3field__box:focus-within {
	outline: 2px solid var(--pc-primary);
	outline-offset: 2px;
	border-radius: var(--pc-radius-field);
}

.m3field--error .m3field__indicator {
	background-color: rgb(var(--pc-danger));
}

.m3field--error .m3field__box:focus-within {
	outline-color: rgb(var(--pc-danger));
}

.m3field--error .m3field__box:focus-within .m3field__indicator {
	background-color: rgb(var(--pc-danger));
}

.m3field--error .m3field__input:focus + .m3field__label,
.m3field--error .m3field__label {
	color: rgb(var(--pc-danger));
}

.m3field--disabled {
	opacity: 0.4;
}

.m3field--disabled .m3field__input {
	cursor: not-allowed;
}

.m3field__msg {
	margin: 0;
	padding: 0 1rem;
	font-size: 0.875rem;
	line-height: 1.25rem;
	color: rgb(var(--pc-muted));
}

.m3field__msg--error {
	color: rgb(var(--pc-danger));
}

/* Reduced motion: fades cap at 100ms, no translation. */
@media (prefers-reduced-motion: reduce) {
	.m3field__box,
	.m3field__label,
	.m3field__indicator {
		transition-duration: var(--pc-duration-shortest);
	}
	.m3field__input:focus + .m3field__label,
	.m3field__input:not(:placeholder-shown) + .m3field__label {
		transform: none;
	}
}

html[data-reduce-motion] .m3field__box,
html[data-reduce-motion] .m3field__label,
html[data-reduce-motion] .m3field__indicator {
	transition-duration: var(--pc-duration-shortest);
}

html[data-reduce-motion] .m3field__input:focus + .m3field__label,
html[data-reduce-motion] .m3field__input:not(:placeholder-shown) + .m3field__label {
	transform: none;
}
</style>
