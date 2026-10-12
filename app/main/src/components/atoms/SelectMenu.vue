<script setup lang="ts">
// Pixel Connect — Material 3 Expressive select menu.
//
// Custom menu scaling in from the anchor; Escape closes and returns focus.
// Full keyboard support (arrows/Enter/Home/End). No native <select>.

import { computed, nextTick, onMounted, onUnmounted, ref, useId } from 'vue'

export interface MenuOption {
	value: string
	label: string
	disabled?: boolean
}

const props = withDefaults(
	defineProps<{
		modelValue: string | null
		options: MenuOption[]
		label?: string
		placeholder?: string
		disabled?: boolean
	}>(),
	{ label: 'Choose an option', placeholder: 'Select…', disabled: false },
)

const emit = defineEmits<{ 'update:modelValue': [string] }>()

const open = ref(false)
const focusedIdx = ref(-1)
const rootRef = ref<HTMLElement | null>(null)
const btnRef = ref<HTMLButtonElement | null>(null)
const listRef = ref<HTMLUListElement | null>(null)
const menuId = useId()

const selected = computed(() => props.options.find((o) => o.value === props.modelValue))

function enabledIndices(): number[] {
	return props.options
		.map((o, i) => (o.disabled ? -1 : i))
		.filter((i) => i >= 0)
}

function toggle() {
	if (props.disabled) return
	open.value ? close() : openMenu()
}

function openMenu() {
	open.value = true
	const enabled = enabledIndices()
	const current = props.options.findIndex((o) => o.value === props.modelValue)
	focusedIdx.value =
		current >= 0 && !props.options[current]?.disabled
			? current
			: (enabled[0] ?? -1)
	nextTick(() => listRef.value?.focus())
}

function close(focusButton = false) {
	open.value = false
	focusedIdx.value = -1
	if (focusButton) btnRef.value?.focus()
}

function choose(i: number) {
	const o = props.options[i]
	if (!o || o.disabled || props.disabled) return
	emit('update:modelValue', o.value)
	close(true)
}

function move(dir: 1 | -1) {
	const enabled = enabledIndices()
	if (!enabled.length) return
	const at = enabled.indexOf(focusedIdx.value)
	const next = enabled[(at + dir + enabled.length) % enabled.length]!
	focusedIdx.value = next
	nextTick(() => {
		listRef.value?.querySelector<HTMLElement>(`[data-idx="${next}"]`)?.focus()
	})
}

function onButtonKey(e: KeyboardEvent) {
	if (e.key === 'ArrowDown' || e.key === 'Enter' || e.key === ' ') {
		e.preventDefault()
		if (!open.value) openMenu()
	} else if (e.key === 'ArrowUp') {
		e.preventDefault()
		if (!open.value) openMenu()
	} else if (e.key === 'Escape' && open.value) {
		e.preventDefault()
		close(true)
	}
}

function onListKey(e: KeyboardEvent) {
	if (e.key === 'Escape') {
		e.preventDefault()
		close(true)
	} else if (e.key === 'ArrowDown') {
		e.preventDefault()
		move(1)
	} else if (e.key === 'ArrowUp') {
		e.preventDefault()
		move(-1)
	} else if (e.key === 'Home') {
		e.preventDefault()
		const enabled = enabledIndices()
		if (enabled.length) {
			focusedIdx.value = enabled[0]!
			nextTick(() => {
				listRef.value?.querySelector<HTMLElement>(`[data-idx="${focusedIdx.value}"]`)?.focus()
			})
		}
	} else if (e.key === 'End') {
		e.preventDefault()
		const enabled = enabledIndices()
		if (enabled.length) {
			focusedIdx.value = enabled[enabled.length - 1]!
			nextTick(() => {
				listRef.value?.querySelector<HTMLElement>(`[data-idx="${focusedIdx.value}"]`)?.focus()
			})
		}
	} else if (e.key === 'Enter' || e.key === ' ') {
		e.preventDefault()
		choose(focusedIdx.value)
	} else if (e.key === 'Tab') {
		close()
	}
}

function onPointerDown(e: PointerEvent) {
	if (rootRef.value && !rootRef.value.contains(e.target as Node)) close()
}

onMounted(() => document.addEventListener('pointerdown', onPointerDown))
onUnmounted(() => document.removeEventListener('pointerdown', onPointerDown))
</script>

<template>
	<div ref="rootRef" class="m3select">
		<button
			ref="btnRef"
			type="button"
			class="m3select__button"
			:aria-haspopup="'listbox'"
			:aria-expanded="open"
			:aria-controls="menuId"
			:aria-label="label"
			:disabled="disabled"
			@click="toggle"
			@keydown="onButtonKey"
		>
			<span class="m3select__value" :class="{ 'm3select__value--empty': !selected }">
				{{ selected?.label ?? placeholder }}
			</span>
			<span class="m3select__chevron" :class="{ 'm3select__chevron--open': open }" aria-hidden="true" />
		</button>
		<ul
			v-if="open"
			:id="menuId"
			ref="listRef"
			class="m3select__menu"
			role="listbox"
			:aria-label="label"
			tabindex="-1"
			@keydown="onListKey"
		>
			<li
				v-for="(o, i) in options"
				:key="o.value"
				:data-idx="i"
				role="option"
				class="m3select__option"
				:class="{ 'm3select__option--active': o.value === modelValue }"
				:aria-selected="o.value === modelValue"
				:aria-disabled="o.disabled || undefined"
				:tabindex="i === focusedIdx ? 0 : -1"
				@pointerenter="focusedIdx = o.disabled ? focusedIdx : i"
				@click="choose(i)"
				@keydown.enter.prevent="choose(i)"
				@keydown.space.prevent="choose(i)"
			>
				{{ o.label }}
			</li>
		</ul>
	</div>
</template>

<style scoped>
.m3select {
	position: relative;
	min-width: 0;
}

.m3select__button {
	display: flex;
	align-items: center;
	gap: 0.75rem;
	width: 100%;
	min-height: 48px;
	padding: 0.625rem 1rem;
	border-radius: var(--pc-radius-field);
	border: 1px solid rgb(var(--pc-outline));
	background-color: rgb(var(--pc-card-alt));
	color: rgb(var(--pc-text));
	font-size: 1rem;
	line-height: 1.5rem;
	cursor: pointer;
	transition:
		border-color var(--pc-duration-short) var(--pc-ease-emphasized),
		background-color var(--pc-duration-short) var(--pc-ease-emphasized);
}

.m3select__button:hover:not(:disabled) {
	border-color: rgb(var(--pc-muted));
}

.m3select__button:focus-visible {
	outline: 2px solid var(--pc-primary);
	outline-offset: 2px;
	border-color: var(--pc-primary);
}

.m3select__button:disabled {
	opacity: 0.4;
	cursor: not-allowed;
}

.m3select__value {
	flex: 1 1 auto;
	min-width: 0;
	overflow: hidden;
	text-overflow: ellipsis;
	white-space: nowrap;
	text-align: left;
}

.m3select__value--empty {
	color: rgb(var(--pc-muted));
}

/* CSS chevron: drawn, not an icon-font glyph. */
.m3select__chevron {
	flex: none;
	width: 0.625rem;
	height: 0.625rem;
	border-right: 2px solid currentColor;
	border-bottom: 2px solid currentColor;
	transform: rotate(45deg) translate(-2px, -2px);
	transition: transform var(--pc-duration-short) var(--pc-ease-emphasized);
}

.m3select__chevron--open {
	transform: rotate(225deg) translate(-2px, -2px);
}

/* Menu scales in from the anchor (top edge). */
.m3select__menu {
	position: absolute;
	top: calc(100% + 4px);
	right: 0;
	left: 0;
	z-index: 50;
	margin: 0;
	padding: 0.375rem;
	list-style: none;
	border-radius: var(--pc-radius-inner);
	border: 1px solid rgb(var(--pc-line));
	background-color: rgb(var(--pc-raised));
	box-shadow: 0 12px 32px rgb(0 0 0 / 0.4);
	transform-origin: top center;
	animation: m3select-in var(--pc-duration-short) var(--pc-ease-decelerate);
}

@keyframes m3select-in {
	from {
		opacity: 0;
		transform: scale(0.96) translateY(-4px);
	}
	to {
		opacity: 1;
		transform: scale(1) translateY(0);
	}
}

.m3select__option {
	display: flex;
	align-items: center;
	min-height: 40px;
	padding: 0.5rem 0.75rem;
	border-radius: 0.75rem;
	font-size: 0.9375rem;
	color: rgb(var(--pc-muted));
	cursor: pointer;
	transition:
		background-color var(--pc-duration-shortest) var(--pc-ease-emphasized),
		color var(--pc-duration-shortest) var(--pc-ease-emphasized);
}

.m3select__option:hover,
.m3select__option:focus-visible {
	background-color: rgb(var(--pc-card-alt));
	color: rgb(var(--pc-text));
	outline: none;
}

.m3select__option--active {
	color: var(--pc-primary);
	font-weight: 600;
}

.m3select__option[aria-disabled='true'] {
	opacity: 0.4;
	cursor: not-allowed;
}

/* Reduced motion: no scale/translation, quick fade only. */
@media (prefers-reduced-motion: reduce) {
	.m3select__menu {
		animation: m3select-fade var(--pc-duration-shortest) ease;
	}
	.m3select__chevron {
		transition-duration: var(--pc-duration-shortest);
	}
}

@keyframes m3select-fade {
	from {
		opacity: 0;
	}
	to {
		opacity: 1;
	}
}

html[data-reduce-motion] .m3select__menu {
	animation: m3select-fade var(--pc-duration-shortest) ease;
}

html[data-reduce-motion] .m3select__chevron {
	transition-duration: var(--pc-duration-shortest);
}
</style>
