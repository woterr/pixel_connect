/* eslint-disable no-undef */
/* eslint-disable @typescript-eslint/no-require-imports */
const resolve = require('path').resolve

// Pixel Connect design tokens.
//
// The palette is derived from the supplied Pixel Connect designs: a near-black
// AMOLED background, a single warm off-white accent used for primary actions and
// active navigation, and a single green used only for "online" state.
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
					DEFAULT: '#0A0A0A',
					raised: '#141414',
					card: '#1A1A1A',
					cardAlt: '#212121',
					line: '#2A2A2A',
					muted: '#8E8E8E',
					faint: '#5F5F5F',
				},
				paper: {
					DEFAULT: '#FFFFFF',
					soft: '#F1F1F1',
					dim: '#E2E2E2',
				},
				online: '#3DDC84',
				warn: '#F5A524',
				danger: '#F4544C',
			},
			borderRadius: {
				card: '1.25rem',
				pill: '2rem',
			},
			fontFamily: {
				display: ['Inter', 'system-ui', 'sans-serif'],
			},
			transitionTimingFunction: {
				emphasized: 'cubic-bezier(0.2, 0, 0, 1)',
			},
			letterSpacing: {
				eyebrow: '0.18em',
			},
		},
	},
	plugins: [],
}