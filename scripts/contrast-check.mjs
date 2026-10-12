// Pixel Connect — WCAG contrast gate for the design-token layer.
//
// Reads the BUILT css (app/main/dist/assets/*.css), reconstructs the token
// set for every theme x accent combination, and asserts WCAG AA: 4.5:1 for
// text pairs, 3:1 for UI pairs. Exits non-zero on any failure.
//
// Run: `node scripts/contrast-check.mjs` (after `npm run vite:build` in
// app/main). Token definitions must stay flat literals — no `var()` chains
// inside `--pc-*` values — or this parser cannot resolve them.

import { readdirSync, readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..')
const DIST_ASSETS = join(ROOT, 'app', 'main', 'dist', 'assets')

const THEMES = ['amoled', 'space-dark', 'simple-dark', 'simple-light', 'white-light', 'system']
const ACCENTS = ['purple', 'blue', 'pink', 'green', 'yellow', 'brown', 'gray', 'white']

const TEXT_MIN = 4.5
const UI_MIN = 3.0

// ------------------------------------------------------------ css parsing ---

// Split top-level blocks: returns [{ media: string|null, selector, body }].
function parseBlocks(css) {
	const blocks = []
	let i = 0
	const n = css.length
	const skipWs = () => {
		while (i < n && /\s/.test(css[i])) i++
	}
	while (i < n) {
		skipWs()
		if (i >= n) break
		if (css.startsWith('@media', i)) {
			const open = css.indexOf('{', i)
			if (open === -1) break
			const media = css.slice(i, open).trim()
			let depth = 1
			let j = open + 1
			while (j < n && depth > 0) {
				if (css[j] === '{') depth++
				else if (css[j] === '}') depth--
				j++
			}
			const inner = css.slice(open + 1, j - 1)
			for (const b of parseBlocks(inner)) blocks.push({ ...b, media })
			i = j
		} else if (css[i] === '@') {
			// @property, @font-face, @view-transition, @keyframes: flat, skip.
			const open = css.indexOf('{', i)
			if (open === -1) break
			let depth = 1
			let j = open + 1
			while (j < n && depth > 0) {
				if (css[j] === '{') depth++
				else if (css[j] === '}') depth--
				j++
			}
			i = j
		} else {
			const open = css.indexOf('{', i)
			if (open === -1) break
			const selector = css.slice(i, open).trim()
			const close = css.indexOf('}', open)
			if (close === -1) break
			blocks.push({ media: null, selector, body: css.slice(open + 1, close) })
			i = close + 1
		}
	}
	return blocks
}

function parseDeclarations(body) {
	const out = {}
	for (const part of body.split(';')) {
		const colon = part.indexOf(':')
		if (colon === -1) continue
		const prop = part.slice(0, colon).trim()
		const value = part.slice(colon + 1).trim()
		if (prop.startsWith('--')) out[prop] = value
	}
	return out
}

function attrValue(selector, name) {
	const m = selector.match(new RegExp(`\\[${name}=([^\\]]+)\\]`))
	if (!m) return null
	return m[1].replace(/^['"]|['"]$/g, '')
}

// Does this selector list contribute token definitions to combo (T, A)?
function selectorApplies(selectorList, theme, accent) {
	return selectorList.split(',').some((raw) => {
		const sel = raw.trim()
		if (sel === ':root' || sel === 'html') return true
		if (!sel.includes('data-theme') && !sel.includes('data-accent')) return false
		const t = attrValue(sel, 'data-theme')
		const a = attrValue(sel, 'data-accent')
		if (t && t !== theme) return false
		if (a && a !== accent) return false
		return true
	})
}

// Only prefers-color-scheme media participates; light branches apply to
// light-scheme evaluations, and there is no dark branch (dark is the base).
function mediaApplies(media, lightScheme) {
	if (media === null) return true
	const m = media.toLowerCase()
	if (!m.includes('prefers-color-scheme')) return false
	if (m.includes('prefers-contrast') || m.includes('prefers-reduced-motion')) return false
	const wantsLight = m.includes('prefers-color-scheme:light') || m.includes('prefers-color-scheme: light')
	return wantsLight === lightScheme
}

// ------------------------------------------------------------------ color ---

function toRgb(value) {
	if (!value) return null
	const v = value.trim().toLowerCase()
	if (v.startsWith('#')) {
		let h = v.slice(1)
		if (h.length === 3) h = [...h].map((c) => c + c).join('')
		if (!/^[0-9a-f]{6}$/.test(h)) return null
		const num = parseInt(h, 16)
		return [(num >> 16) & 255, (num >> 8) & 255, num & 255]
	}
	const m = v.match(/^(\d+(?:\.\d+)?)\s+(\d+(?:\.\d+)?)\s+(\d+(?:\.\d+)?)$/)
	if (m) return [Number(m[1]), Number(m[2]), Number(m[3])]
	return null
}

function linear(v) {
	v /= 255
	return v <= 0.04045 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4)
}

function luminance([r, g, b]) {
	return 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
}

function ratio(fg, bg) {
	const [x, y] = [luminance(fg), luminance(bg)].sort((p, q) => q - p)
	return (x + 0.05) / (y + 0.05)
}

// ------------------------------------------------------------------- main ---

function loadBuiltCss() {
	let files
	try {
		files = readdirSync(DIST_ASSETS).filter((f) => f.endsWith('.css'))
	} catch {
		console.error(`error: no built css in ${DIST_ASSETS} — run \`npm run vite:build\` in app/main first`)
		process.exit(2)
	}
	if (!files.length) {
		console.error(`error: no built css in ${DIST_ASSETS} — run \`npm run vite:build\` in app/main first`)
		process.exit(2)
	}
	return files.map((f) => readFileSync(join(DIST_ASSETS, f), 'utf8')).join('\n')
}

// [tokenA, tokenB, floor, label]
const CHECKS = [
	['--pc-text', '--pc-bg', TEXT_MIN, 'text/bg'],
	['--pc-text', '--pc-card', TEXT_MIN, 'text/card'],
	['--pc-muted', '--pc-bg', TEXT_MIN, 'muted/bg'],
	['--pc-muted', '--pc-card', TEXT_MIN, 'muted/card'],
	['--pc-faint', '--pc-bg', TEXT_MIN, 'faint/bg'],
	['--pc-faint', '--pc-card', TEXT_MIN, 'faint/card'],
	['--pc-on-primary', '--pc-primary', TEXT_MIN, 'on-primary/primary'],
	['--pc-on-primary-container', '--pc-primary-container', TEXT_MIN, 'on-container/primary-container'],
	['--pc-on-secondary-container', '--pc-secondary-container', TEXT_MIN, 'on-secondary/secondary-container'],
	['--pc-online', '--pc-bg', TEXT_MIN, 'online/bg'],
	['--pc-online', '--pc-card', TEXT_MIN, 'online/card'],
	['--pc-warn', '--pc-bg', TEXT_MIN, 'warn/bg'],
	['--pc-warn', '--pc-card', TEXT_MIN, 'warn/card'],
	['--pc-danger', '--pc-bg', TEXT_MIN, 'danger/bg'],
	['--pc-danger', '--pc-card', TEXT_MIN, 'danger/card'],
	['--pc-primary', '--pc-bg', UI_MIN, 'primary/bg (UI)'],
	['--pc-outline', '--pc-bg', UI_MIN, 'outline/bg (UI)'],
]

function tokensFor(blocks, theme, accent, lightScheme) {
	const tokens = {}
	for (const b of blocks) {
		if (!mediaApplies(b.media, lightScheme)) continue
		if (!selectorApplies(b.selector, theme, accent)) continue
		Object.assign(tokens, parseDeclarations(b.body))
	}
	return tokens
}

const LIGHT_THEMES = new Set(['simple-light', 'white-light'])

const css = loadBuiltCss()
const blocks = parseBlocks(css)

let combos = 0
let assertions = 0
let failures = []
let worst = null

for (const theme of THEMES) {
	const schemes = theme === 'system' ? [false, true] : [LIGHT_THEMES.has(theme)]
	for (const accent of ACCENTS) {
		combos++
		for (const lightScheme of schemes) {
			const tokens = tokensFor(blocks, theme, accent, lightScheme)
			const tag = schemes.length > 1 ? `${theme}:${lightScheme ? 'light' : 'dark'}` : theme
			const missing = [...new Set(CHECKS.flatMap(([a, b]) => [a, b]))].filter(
				(t) => !toRgb(tokens[t]),
			)
			if (missing.length) {
				failures.push(`${tag} x ${accent}: unresolvable tokens: ${missing.join(', ')}`)
				continue
			}
			for (const [ta, tb, floor, label] of CHECKS) {
				assertions++
				const r = ratio(toRgb(tokens[ta]), toRgb(tokens[tb]))
				const margin = r - floor
				if (!worst || margin < worst.margin) worst = { margin, combo: `${tag} x ${accent}`, label, r }
				if (r < floor) failures.push(`${tag} x ${accent}: ${label} = ${r.toFixed(2)}:1 (needs ${floor}:1)`)
			}
		}
	}
}

console.log(`contrast-check: ${combos} combos (6 themes x 8 accents; system in both schemes), ${assertions} assertions`)
if (worst) console.log(`thinnest margin: ${worst.combo} ${worst.label} = ${worst.r.toFixed(2)}:1 (+${worst.margin.toFixed(2)})`)
if (failures.length) {
	console.error(`\n${failures.length} FAILURES:`)
	for (const f of failures) console.error(`  ${f}`)
	process.exit(1)
}
console.log('all contrast checks passed')
