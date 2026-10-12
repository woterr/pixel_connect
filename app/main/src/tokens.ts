// Pixel Connect — design-token source of truth.
//
// Everything visual derives from here: the six themes (`data-theme`), the
// eight accents (`data-accent`), and the motion/type/shape/spacing scales.
// `assets/main.css` carries the same values as flat custom-property literals
// (no `var()` chains in token definitions) so `scripts/contrast-check.mjs`
// can verify the built CSS without a resolver. If a value changes here, copy
// it into `main.css`; the contrast script fails the build otherwise.
//
// The backend (`src-tauri/src/settings.rs`) only persists the choice of
// theme/accent/reduce-motion; the seeds and the mapping live here.

// ---------------------------------------------------------------- seeds ---

export const ACCENT_IDS = [
	'purple',
	'blue',
	'pink',
	'green',
	'yellow',
	'brown',
	'gray',
	'white',
] as const

export type AccentId = (typeof ACCENT_IDS)[number]

/** Seed colour for each `data-accent`. The backend default is `green`. */
export const ACCENT_SEEDS: Record<AccentId, string> = {
	purple: '#9A7BFF',
	blue: '#4C8DF6',
	pink: '#F0629F',
	green: '#3DDC84',
	yellow: '#F9C440',
	brown: '#A98274',
	gray: '#9AA0A6',
	white: '#FFFFFF',
}

/** Neutral seed reused for the monochrome (`white`) accent's containers. */
export const NEUTRAL_SEED = '#9AA0A6'

export const THEME_IDS = [
	'amoled',
	'space-dark',
	'simple-dark',
	'simple-light',
	'white-light',
	'system',
] as const

export type ThemeId = (typeof THEME_IDS)[number]

/**
 * Stored values that no longer name a theme of their own but keep working:
 * `dark`->`simple-dark`, `light`->`simple-light`, `midnight`->`space-dark`,
 * plus the two high-contrast themes, which now alias the nearest new theme.
 */
export const LEGACY_THEME_ALIAS: Record<string, ThemeId> = {
	dark: 'simple-dark',
	light: 'simple-light',
	midnight: 'space-dark',
	'contrast-dark': 'amoled',
	'contrast-light': 'white-light',
}

// ------------------------------------------------------------- tonal math ---

/** `RRGGBB` (or short `RGB`) hex to `[r, g, b]`. */
export function hexToRgb(hex: string): [number, number, number] {
	let h = hex.replace('#', '')
	if (h.length === 3) h = [...h].map((c) => c + c).join('')
	const n = parseInt(h, 16)
	return [(n >> 16) & 255, (n >> 8) & 255, n & 255]
}

export function rgbToHex(r: number, g: number, b: number): string {
	const c = (v: number) =>
		Math.round(Math.min(255, Math.max(0, v)))
			.toString(16)
			.padStart(2, '0')
	// Uppercase to match the literals in main.css byte-for-byte.
	return `#${c(r)}${c(g)}${c(b)}`.toUpperCase()
}

/** `[r, g, b]` triplet as used inside `rgb(var(--pc-*))`. */
export function hexToTriplet(hex: string): string {
	return hexToRgb(hex).join(' ')
}

function rgbToHsl(r: number, g: number, b: number): [number, number, number] {
	r /= 255
	g /= 255
	b /= 255
	const max = Math.max(r, g, b)
	const min = Math.min(r, g, b)
	const l = (max + min) / 2
	if (max === min) return [0, 0, l]
	const d = max - min
	const s = l > 0.5 ? d / (2 - max - min) : d / (max + min)
	let h = 0
	if (max === r) h = (g - b) / d + (g < b ? 6 : 0)
	else if (max === g) h = (b - r) / d + 2
	else h = (r - g) / d + 4
	return [h * 60, s, l]
}

function hslToRgb(h: number, s: number, l: number): [number, number, number] {
	h = ((((h % 360) + 360) % 360) / 360) as number
	const q = l < 0.5 ? l * (1 + s) : l + s - l * s
	const p = 2 * l - q
	const f = (t: number): number => {
		t = ((t % 1) + 1) % 1
		if (t < 1 / 6) return p + (q - p) * 6 * t
		if (t < 1 / 2) return q
		if (t < 2 / 3) return p + (q - p) * (2 / 3 - t) * 6
		return p
	}
	return [f(h + 1 / 3) * 255, f(h) * 255, f(h - 1 / 3) * 255]
}

/**
 * Tonal step `0..100` of a seed: the seed's hue and saturation with lightness
 * pinned to the tone. Tone 0 is black and tone 100 is white whatever the hue,
 * so a near-white seed (`white`, `gray`) degrades to a neutral grey ramp.
 */
export function tone(seedHex: string, t: number): string {
	const [r, g, b] = hexToRgb(seedHex)
	const [h, s] = rgbToHsl(r, g, b)
	const [rr, gg, bb] = hslToRgb(h, s, t / 100)
	return rgbToHex(rr, gg, bb)
}

function linear(v: number): number {
	v /= 255
	return v <= 0.04045 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4)
}

/** WCAG relative luminance of a hex colour. */
export function luminance(hex: string): number {
	const [r, g, b] = hexToRgb(hex)
	return 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
}

/** WCAG contrast ratio between two hex colours. */
export function contrastRatio(a: string, b: string): number {
	const [x, y] = [luminance(a), luminance(b)].sort((p, q) => q - p)
	return (x + 0.05) / (y + 0.05)
}

export interface AccentRoles {
	primary: string
	onPrimary: string
	primaryContainer: string
	onPrimaryContainer: string
	secondaryContainer: string
	onSecondaryContainer: string
	surfaceTint: string
}

/**
 * Text colour for `bgHex` reaching WCAG AA (4.5:1), trying the preferred
 * family first. This is what gives yellow/brown their darker text on light
 * schemes: white simply fails there, so a near-black is picked instead.
 */
export function pickOnColor(
	seedHex: string,
	bgHex: string,
	preferLightText: boolean,
): string {
	const light = ['#FFFFFF', tone(seedHex, 95), tone(seedHex, 90)]
	const dark = ['#0C0C0D', tone(seedHex, 12), tone(seedHex, 10), '#000000']
	const order = preferLightText ? [...light, ...dark] : [...dark, ...light]
	for (const c of order) if (contrastRatio(c, bgHex) >= 4.5) return c
	return order
		.map((c) => [c, contrastRatio(c, bgHex)] as const)
		.sort((a, b) => b[1] - a[1])[0][0]
}

// ------------------------------------------------------------ role tables ---

/**
 * Per-scheme primary tone. The spec mapping is dark -> tone 80 and light ->
 * tone 40; green and yellow keep tones 36 on light because tone 40 misses
 * the 3:1 UI floor against near-white surfaces (2.6-2.8:1). Nearest passing
 * tones, with margin kept off the exact floor.
 */
const LIGHT_PRIMARY_TONE: Record<AccentId, number> = {
	purple: 40,
	blue: 40,
	pink: 40,
	green: 36,
	yellow: 36,
	brown: 40,
	gray: 40,
	white: 40, // Unused: white is special-cased below.
}

/**
 * Generated tonal roles for one accent on one scheme.
 *
 * Containers follow the spec mapping exactly (dark: 30/25, light: 90/95).
 * `white` is the inverse accent: a light primary on dark schemes and a
 * near-black primary on light ones, with neutral-grey containers. `gray`
 * needs no special case: its seed is already near-neutral.
 */
export function rolesFor(accent: AccentId, scheme: 'dark' | 'light'): AccentRoles {
	const dark = scheme === 'dark'
	if (accent === 'white') {
		if (dark) {
			const primary = '#FFFFFF'
			const primaryContainer = tone(NEUTRAL_SEED, 30)
			const secondaryContainer = tone(NEUTRAL_SEED, 25)
			return {
				primary,
				onPrimary: '#0B0B0C',
				primaryContainer,
				onPrimaryContainer: pickOnColor(NEUTRAL_SEED, primaryContainer, true),
				secondaryContainer,
				onSecondaryContainer: pickOnColor(NEUTRAL_SEED, secondaryContainer, true),
				surfaceTint: primary,
			}
		}
		const primary = '#17171A'
		const primaryContainer = tone(NEUTRAL_SEED, 90)
		const secondaryContainer = tone(NEUTRAL_SEED, 95)
		return {
			primary,
			onPrimary: '#FFFFFF',
			primaryContainer,
			onPrimaryContainer: pickOnColor(NEUTRAL_SEED, primaryContainer, false),
			secondaryContainer,
			onSecondaryContainer: pickOnColor(NEUTRAL_SEED, secondaryContainer, false),
			surfaceTint: primary,
		}
	}
	const seed = ACCENT_SEEDS[accent]
	const primary = dark ? tone(seed, 80) : tone(seed, LIGHT_PRIMARY_TONE[accent])
	const primaryContainer = dark ? tone(seed, 30) : tone(seed, 90)
	const secondaryContainer = dark ? tone(seed, 25) : tone(seed, 95)
	return {
		primary,
		onPrimary: pickOnColor(seed, primary, dark),
		primaryContainer,
		onPrimaryContainer: pickOnColor(seed, primaryContainer, dark),
		secondaryContainer,
		onSecondaryContainer: pickOnColor(seed, secondaryContainer, dark),
		surfaceTint: primary,
	}
}

/**
 * Precomputed roles (hex) for every accent x scheme. These literals are what
 * `main.css` carries; `rolesFor` above is the procedure that produced them.
 */
export const ACCENT_ROLES: Record<'dark' | 'light', Record<AccentId, AccentRoles>> = {
	dark: {
		purple: { primary: '#B199FF', onPrimary: '#0C0C0D', primaryContainer: '#240099', onPrimaryContainer: '#FFFFFF', secondaryContainer: '#1E0080', onSecondaryContainer: '#FFFFFF', surfaceTint: '#B199FF' },
		blue: { primary: '#9EC1FA', onPrimary: '#0C0C0D', primaryContainer: '#073C92', onPrimaryContainer: '#FFFFFF', secondaryContainer: '#063279', onSecondaryContainer: '#FFFFFF', surfaceTint: '#9EC1FA' },
		pink: { primary: '#F6A2C6', onPrimary: '#0C0C0D', primaryContainer: '#8C0D44', onPrimaryContainer: '#FFFFFF', secondaryContainer: '#740B38', onSecondaryContainer: '#FFFFFF', surfaceTint: '#F6A2C6' },
		green: { primary: '#A9EFC8', onPrimary: '#0C0C0D', primaryContainer: '#178247', onPrimaryContainer: '#FFFFFF', secondaryContainer: '#136C3B', onSecondaryContainer: '#FFFFFF', surfaceTint: '#A9EFC8' },
		yellow: { primary: '#FCE09C', onPrimary: '#0C0C0D', primaryContainer: '#946B05', onPrimaryContainer: '#FFFFFF', secondaryContainer: '#7C5904', onSecondaryContainer: '#FFFFFF', surfaceTint: '#FCE09C' },
		brown: { primary: '#D8C6C0', onPrimary: '#0C0C0D', primaryContainer: '#5F443A', onPrimaryContainer: '#FFFFFF', secondaryContainer: '#4F3931', onSecondaryContainer: '#FFFFFF', surfaceTint: '#D8C6C0' },
		gray: { primary: '#C9CCCF', onPrimary: '#0C0C0D', primaryContainer: '#484C51', onPrimaryContainer: '#FFFFFF', secondaryContainer: '#3C4044', onSecondaryContainer: '#FFFFFF', surfaceTint: '#C9CCCF' },
		white: { primary: '#FFFFFF', onPrimary: '#0B0B0C', primaryContainer: '#484C51', onPrimaryContainer: '#FFFFFF', secondaryContainer: '#3C4044', onSecondaryContainer: '#FFFFFF', surfaceTint: '#FFFFFF' },
	},
	light: {
		purple: { primary: '#3000CC', onPrimary: '#FFFFFF', primaryContainer: '#D8CCFF', onPrimaryContainer: '#0C0C0D', secondaryContainer: '#EBE5FF', onSecondaryContainer: '#0C0C0D', surfaceTint: '#3000CC' },
		blue: { primary: '#0A50C2', onPrimary: '#FFFFFF', primaryContainer: '#CEE0FD', onPrimaryContainer: '#0C0C0D', secondaryContainer: '#E7F0FE', onSecondaryContainer: '#0C0C0D', surfaceTint: '#0A50C2' },
		pink: { primary: '#BA125A', onPrimary: '#FFFFFF', primaryContainer: '#FBD0E3', onPrimaryContainer: '#0C0C0D', secondaryContainer: '#FDE8F1', onSecondaryContainer: '#0C0C0D', surfaceTint: '#BA125A' },
		green: { primary: '#1C9C55', onPrimary: '#0C0C0D', primaryContainer: '#D4F7E4', onPrimaryContainer: '#0C0C0D', secondaryContainer: '#E9FBF1', onSecondaryContainer: '#0C0C0D', surfaceTint: '#1C9C55' },
		yellow: { primary: '#B28106', onPrimary: '#0C0C0D', primaryContainer: '#FDF0CE', onPrimaryContainer: '#0C0C0D', secondaryContainer: '#FEF7E6', onSecondaryContainer: '#0C0C0D', surfaceTint: '#B28106' },
		brown: { primary: '#7E5B4E', onPrimary: '#FFFFFF', primaryContainer: '#ECE3DF', onPrimaryContainer: '#0C0C0D', secondaryContainer: '#F5F1EF', onSecondaryContainer: '#0C0C0D', surfaceTint: '#7E5B4E' },
		gray: { primary: '#60666C', onPrimary: '#FFFFFF', primaryContainer: '#E4E6E7', onPrimaryContainer: '#0C0C0D', secondaryContainer: '#F1F2F3', onSecondaryContainer: '#0C0C0D', surfaceTint: '#60666C' },
		white: { primary: '#17171A', onPrimary: '#FFFFFF', primaryContainer: '#E4E6E7', onPrimaryContainer: '#0C0C0D', secondaryContainer: '#F1F2F3', onSecondaryContainer: '#0C0C0D', surfaceTint: '#17171A' },
	},
}

// --------------------------------------------------------------- surfaces ---

/** Surface + status triplets per theme, as `r g b` literals for main.css. */
export interface ThemeSurfaces {
	bg: string
	raised: string
	card: string
	cardAlt: string
	/** Decorative hairlines only; never meaningful boundaries. */
	line: string
	/** Meaningful borders (dropzones, focus-adjacent chrome). >= 3:1 vs bg. */
	outline: string
	text: string
	muted: string
	faint: string
	online: string
	warn: string
	danger: string
}

export const THEME_SURFACES: Record<ThemeId, ThemeSurfaces> = {
	amoled: { bg: '0 0 0', raised: '10 10 10', card: '20 20 20', cardAlt: '32 32 32', line: '42 42 42', outline: '117 117 117', text: '244 244 244', muted: '174 174 174', faint: '154 154 154', online: '61 220 132', warn: '245 165 36', danger: '244 84 76' },
	'space-dark': { bg: '10 12 22', raised: '16 19 34', card: '21 26 46', cardAlt: '28 35 64', line: '42 51 85', outline: '138 148 184', text: '236 239 247', muted: '169 178 204', faint: '147 155 184', online: '61 220 132', warn: '245 165 36', danger: '244 84 76' },
	'simple-dark': { bg: '19 19 19', raised: '27 27 27', card: '33 33 33', cardAlt: '41 41 41', line: '46 46 46', outline: '123 123 123', text: '244 244 244', muted: '176 176 176', faint: '156 156 156', online: '61 220 132', warn: '245 165 36', danger: '244 84 76' },
	'simple-light': { bg: '245 245 243', raised: '255 255 255', card: '255 255 255', cardAlt: '236 236 234', line: '223 223 220', outline: '111 111 108', text: '22 22 20', muted: '74 74 70', faint: '91 91 87', online: '19 125 76', warn: '156 96 0', danger: '199 42 34' },
	'white-light': { bg: '255 255 255', raised: '255 255 255', card: '255 255 255', cardAlt: '241 241 241', line: '226 226 226', outline: '112 112 112', text: '10 10 10', muted: '69 69 69', faint: '89 89 89', online: '19 125 76', warn: '156 96 0', danger: '199 42 34' },
	system: { bg: '19 19 19', raised: '27 27 27', card: '33 33 33', cardAlt: '41 41 41', line: '46 46 46', outline: '123 123 123', text: '244 244 244', muted: '176 176 176', faint: '156 156 156', online: '61 220 132', warn: '245 165 36', danger: '244 84 76' },
}

// ------------------------------------------------------------------ scales ---

/** Motion tokens (mirrored as `--pc-*` custom properties in main.css). */
export const MOTION = {
	easeEmphasized: 'cubic-bezier(0.2, 0, 0, 1)',
	easeDecelerate: 'cubic-bezier(0.05, 0.7, 0.1, 1)',
	easeAccelerate: 'cubic-bezier(0.3, 0, 0.8, 0.15)',
	durationShortest: 100,
	durationShort: 200,
	durationMedium: 300,
	durationLong: 500,
	/** Gentle overshoot for toggles and chips; never for navigation. */
	springSoft: 'cubic-bezier(0.34, 1.3, 0.64, 1)',
	springSnappy: 'cubic-bezier(0.3, 1.4, 0.5, 1)',
} as const

/** Type scale: size/line pairs in px, with the enforced floors. */
export const TYPE_SCALE = {
	display: { size: 36, line: 44, weight: 800, tracking: '-0.02em' },
	headline: { size: 28, line: 36, weight: 700, tracking: '-0.01em' },
	title: { size: 20, line: 28, weight: 600, tracking: '0' },
	bodyL: { size: 16, line: 24, weight: 400, tracking: '0' },
	bodyM: { size: 14, line: 20, weight: 400, tracking: '0' },
	label: { size: 12, line: 16, weight: 500, tracking: '0.04em' },
	/** Body copy never drops below 14px; supporting text never below 13px. */
	floorBody: 14,
	floorSupporting: 13,
} as const

export const SPACING = {
	grid: 4,
	contentMax: 880,
	contentPad: 32,
} as const

export const SHAPE = {
	card: 24,
	inner: 16,
	field: 16,
	pill: 9999,
} as const

/** State-layer opacities over the surface tint. */
export const STATE_LAYERS = {
	hover: 0.08,
	focus: 0.12,
	pressed: 0.12,
	dragged: 0.16,
} as const

// ------------------------------------------------- pre-paint (documented) ---
//
// No flash on launch. The runtime application of theme/accent/reduce-motion
// already exists as `stores/app.ts applyTheme` (settings -> documentElement
// attributes at bootstrap); this snippet only extends that pattern to the
// first paint, which happens before the backend can answer. It reads the
// localStorage cache that `applyTheme` maintains (see below) and sets the
// same attributes, so the very first frame already matches the stored choice.
//
// NOT installed in index.html yet: installing it is a follow-up owned by the
// presentation wiring (it must stay in step with whatever applyTheme caches).
// Kept here so the two cannot drift apart unnoticed.
//
// ```html
// <script>
// // Pixel Connect pre-paint: mirror of stores/app.ts applyTheme.
// try {
//   var t = localStorage.getItem('pc-theme');
//   var a = localStorage.getItem('pc-accent');
//   var r = localStorage.getItem('pc-reduce-motion');
//   if (t) document.documentElement.setAttribute('data-theme', t);
//   if (a) document.documentElement.setAttribute('data-accent', a);
//   if (r === '1') document.documentElement.setAttribute('data-reduce-motion', '');
// } catch (e) { /* private mode: fall back to :root defaults */ }
// </script>
// ```
//
// The `applyTheme` side of the contract (for whoever extends it):
// after writing the attributes, cache them with
// `localStorage.setItem('pc-theme', name)` / `('pc-accent', accent)` /
// `('pc-reduce-motion', reduceMotion ? '1' : '0')`, each in try/catch.
// Unknown stored values must fall back (theme `dark`->simple-dark surfaces,
// accent `green`) rather than being applied blindly, matching the backend's
// `THEMES`/`ACCENTS` validation in `src-tauri/src/settings.rs`.
export const PRE_PAINT_CACHE_KEYS = {
	theme: 'pc-theme',
	accent: 'pc-accent',
	reduceMotion: 'pc-reduce-motion',
} as const
