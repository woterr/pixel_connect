// Pixel Connect — formatting helpers.
//
// These drive what the user reads on the transfer screen, so the edge cases
// (unknown sizes, sub-second ETAs, stale timestamps) are pinned down here.

import { describe, expect, it } from 'vitest'

import {
	deviceTypeLabel,
	etaFrom,
	fileCategory,
	fileExtension,
	formatBytes,
	formatDuration,
	formatEta,
	formatRate,
	pluralise,
	timeAgo,
	toPercent,
	truncate,
} from '../../src/utils'

describe('formatBytes', () => {
	it('shows bytes below a kilobyte', () => {
		expect(formatBytes(0)).toBe('0 B')
		expect(formatBytes(512)).toBe('512 B')
	})

	it('uses one decimal below ten, none above', () => {
		expect(formatBytes(12.4 * 1024)).toBe('12 KB')
		expect(formatBytes(1024)).toBe('1.0 KB')
		expect(formatBytes(5 * 1024 * 1024)).toBe('5.0 MB')
		expect(formatBytes(1500 * 1024 * 1024)).toBe('1.5 GB')
	})

	it('renders an em dash for unknown sizes', () => {
		expect(formatBytes(null)).toBe('—')
		expect(formatBytes(undefined)).toBe('—')
		expect(formatBytes(Number.NaN)).toBe('—')
	})
})

describe('toPercent', () => {
	it('clamps to 0..100', () => {
		expect(toPercent(0)).toBe(0)
		expect(toPercent(0.644)).toBe(64)
		expect(toPercent(1)).toBe(100)
		expect(toPercent(1.5)).toBe(100)
		expect(toPercent(-0.2)).toBe(0)
		expect(toPercent(Number.NaN)).toBe(0)
	})
})

describe('formatEta', () => {
	it('uses seconds below a minute', () => {
		expect(formatEta(2)).toBe('2s left')
		expect(formatEta(0)).toBe('0s left')
	})

	it('uses minutes and seconds above a minute', () => {
		expect(formatEta(80)).toBe('1m 20s remaining')
		expect(formatEta(120)).toBe('2m remaining')
	})

	it('handles hours', () => {
		expect(formatEta(3700)).toBe('1h 1m remaining')
	})

	it('is empty when unknown', () => {
		expect(formatEta(null)).toBe('')
		expect(formatEta(-1)).toBe('')
	})
})

describe('etaFrom', () => {
	it('extrapolates from elapsed time', () => {
		// A quarter done after 2s means ~8s total, so ~6s remain.
		expect(Math.round(etaFrom(0.25, 2000)!)).toBe(6)
	})

	it('is unknown at the extremes', () => {
		expect(etaFrom(0, 2000)).toBeNull()
		expect(etaFrom(1, 2000)).toBeNull()
		expect(etaFrom(0.5, 0)).toBeNull()
	})
})

describe('timeAgo', () => {
	const now = Math.floor(Date.now() / 1000)
	it('describes recent and past times', () => {
		expect(timeAgo(now - 2)).toBe('now')
		expect(timeAgo(now - 30)).toBe('30s ago')
		expect(timeAgo(now - 300)).toBe('5m ago')
		expect(timeAgo(now - 7200)).toBe('2h ago')
		expect(timeAgo(now - 172800)).toBe('2d ago')
	})

	it('reports never for missing values', () => {
		expect(timeAgo(null)).toBe('never')
		expect(timeAgo(0)).toBe('never')
	})
})

describe('file helpers', () => {
	it('extracts extensions', () => {
		expect(fileExtension('a.tar.gz')).toBe('gz')
		expect(fileExtension('README')).toBe('')
		expect(fileExtension(null)).toBe('')
	})

	it('categorises by extension', () => {
		expect(fileCategory('a.pdf')).toBe('doc')
		expect(fileCategory('a.PNG')).toBe('image')
		expect(fileCategory('a.zip')).toBe('archive')
		expect(fileCategory('a.mp4')).toBe('media')
		expect(fileCategory('a.rs')).toBe('code')
		expect(fileCategory('a.unknownext')).toBe('generic')
	})
})

describe('misc', () => {
	it('labels device types', () => {
		expect(deviceTypeLabel('phone')).toBe('Phone')
		expect(deviceTypeLabel('tablet')).toBe('Tablet')
		expect(deviceTypeLabel('laptop')).toBe('Laptop')
		expect(deviceTypeLabel('tv')).toBe('TV')
		expect(deviceTypeLabel('something-else')).toBe('Desktop')
	})

	it('formats rates and durations', () => {
		expect(formatRate(0)).toBe('—')
		expect(formatRate(1024 * 1024)).toBe('1.0 MB/s')
		expect(formatDuration(45)).toBe('45s')
		expect(formatDuration(300)).toBe('5m')
		expect(formatDuration(7200)).toBe('2h')
		expect(formatDuration(172800)).toBe('2d')
	})

	it('truncates and pluralises', () => {
		expect(truncate('short', 10)).toBe('short')
		expect(truncate('abcdefghij', 5)).toBe('abcd…')
		expect(pluralise(1, 'file')).toBe('file')
		expect(pluralise(2, 'file')).toBe('files')
		expect(pluralise(2, 'entry', 'entries')).toBe('entries')
	})
})