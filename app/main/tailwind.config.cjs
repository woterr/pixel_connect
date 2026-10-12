/* eslint-disable no-undef */
/* eslint-disable @typescript-eslint/no-require-imports */
const resolve = require('path').resolve

// Pixel Connect design tokens.
//
// Every color resolves to a CSS variable from `src/assets/main.css` (single
// source of truth: `src/tokens.ts`), so Tailwind classes follow `data-theme`
// and `data-accent` instead of bypassing them. The `<alpha-value>` placeholder
// keeps opacity modifiers (`bg-warn/10`) working with the variable form.
//
// Name mapping (kept stable so pre-migration classes keep working):
//   ink.*    surfaces and text chrome (--pc-bg, --pc-card, --pc-line, ...)
//   paper.*  light-plate accents; DEFAULT is the text colour (borders that
//            follow the text, e.g. `border-l-paper`)
//   online/warn/danger  status (--pc-online/--pc-warn/--pc-danger)
//   accent.* the active accent roles (--pc-primary, ...)
// NOTE: `bg-ink` is the canvas while `text-ink` is the text colour — one
// palette entry cannot be both, so `text-ink` is overridden to --pc-text in
// main.css's compatibility utilities. Prefer explicit `bg-ink`/`text-ink`
// pairs over bare `text-ink` in new code.
//
// Type floor: `text-xs` is 13px, not 12px. Supporting text never drops below
// 13px and body copy never below 14px (`text-sm`); the 12px Label role exists
// only as `.pc-label` / `.pc-eyebrow` / `.pc-pill` / `.pc-chip`.
const withAlpha = (variable) => `rgb(var(${variable}) / <alpha-value>)`

module.exports = {
	content: [
		resolve(__dirname, 'index.html'),
		resolve(__dirname, 'src/**/*.{vue,ts}'),
	],
	darkMode: 'class',
	theme: {
		extend: {
			colors: {
				ink: {
					DEFAULT: withAlpha('--pc-bg'),
					raised: withAlpha('--pc-raised'),
					card: withAlpha('--pc-card'),
					cardAlt: withAlpha('--pc-card-alt'),
					line: withAlpha('--pc-line'),
					muted: withAlpha('--pc-muted'),
					faint: withAlpha('--pc-faint'),
				},
				paper: {
					DEFAULT: withAlpha('--pc-text'),
					soft: withAlpha('--pc-card-alt'),
					dim: withAlpha('--pc-line'),
				},
				online: withAlpha('--pc-online'),
				warn: withAlpha('--pc-warn'),
				danger: withAlpha('--pc-danger'),
				accent: {
					DEFAULT: 'var(--pc-primary)',
					onAccent: 'var(--pc-on-primary)',
					container: 'var(--pc-primary-container)',
					onContainer: 'var(--pc-on-primary-container)',
					secondaryContainer: 'var(--pc-secondary-container)',
					tint: 'var(--pc-surface-tint)',
				},
			},
			borderRadius: {
				card: '1.5rem',
				inner: '1rem',
				field: '1rem',
				pill: '9999px',
			},
			fontFamily: {
				sans: [
					'Google Sans Flex',
					'Inter',
					'system-ui',
					'-apple-system',
					'Segoe UI',
					'Roboto',
					'sans-serif',
				],
				display: [
					'Google Sans Flex',
					'Inter',
					'system-ui',
					'sans-serif',
				],
			},
			fontSize: {
				xs: ['0.8125rem', '1.25rem'],
			},
			transitionTimingFunction: {
				emphasized: 'cubic-bezier(0.2, 0, 0, 1)',
				decelerate: 'cubic-bezier(0.05, 0.7, 0.1, 1)',
				accelerate: 'cubic-bezier(0.3, 0, 0.8, 0.15)',
			},
			letterSpacing: {
				eyebrow: '0.18em',
			},
			maxWidth: {
				content: '880px',
			},
		},
	},
	plugins: [],
}
