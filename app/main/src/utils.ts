// Pixel Connect — small formatting helpers shared by the views.

/** Human readable byte count, e.g. "1.2 GB". */
export function formatBytes(bytes: number | null | undefined): string {
	if (bytes === null || bytes === undefined || Number.isNaN(bytes)) return '—'
	if (bytes < 1024) return `${bytes} B`
	const units = ['KB', 'MB', 'GB', 'TB']
	let value = bytes / 1024
	let unit = 0
	while (value >= 1024 && unit < units.length - 1) {
		value /= 1024
		unit++
	}
	return `${value.toFixed(value < 10 ? 1 : 0)} ${units[unit]}`
}

/** Percentage as an integer 0..100. */
export function toPercent(ratio: number): number {
	if (!Number.isFinite(ratio)) return 0
	return Math.max(0, Math.min(100, Math.round(ratio * 100)))
}

/** "2s left", "45s remaining", "1m 20s remaining". */
export function formatEta(seconds: number | null | undefined): string {
	if (seconds === null || seconds === undefined || seconds < 0) return ''
	if (seconds < 60) return `${Math.ceil(seconds)}s left`
	const m = Math.floor(seconds / 60)
	const s = Math.ceil(seconds % 60)
	if (m < 60) return s ? `${m}m ${s}s remaining` : `${m}m remaining`
	const h = Math.floor(m / 60)
	return `${h}h ${m % 60}m remaining`
}

/** Seconds left given progress and elapsed time, or null if unknown. */
export function etaFrom(
	ratio: number,
	elapsedMs: number,
): number | null {
	if (ratio <= 0.01 || ratio >= 1 || elapsedMs <= 0) return null
	const totalMs = elapsedMs / ratio
	return Math.max(0, (totalMs - elapsedMs) / 1000)
}

export function formatDuration(seconds: number): string {
	if (seconds < 60) return `${Math.max(1, Math.round(seconds))}s`
	if (seconds < 3600) return `${Math.round(seconds / 60)}m`
	if (seconds < 86400) return `${Math.round(seconds / 3600)}h`
	return `${Math.round(seconds / 86400)}d`
}

/** Relative time for "2m ago" style labels. */
export function timeAgo(unixSeconds: number | null | undefined): string {
	if (!unixSeconds) return 'never'
	// A timestamp in the future means the peer's clock is ahead, or one was
	// written before ours caught up. Showing a negative "in Ns" would be
	// nonsense, so clamp to zero rather than rendering something absurd.
	const delta = Math.max(0, Math.floor(Date.now() / 1000) - unixSeconds)
	if (delta < 5) return 'now'
	if (delta < 60) return `${delta}s ago`
	if (delta < 3600) return `${Math.floor(delta / 60)}m ago`
	if (delta < 86400) return `${Math.floor(delta / 3600)}h ago`
	return `${Math.floor(delta / 86400)}d ago`
}

export function formatRate(bytesPerSecond: number): string {
	if (!Number.isFinite(bytesPerSecond) || bytesPerSecond <= 0) return '—'
	return `${formatBytes(bytesPerSecond)}/s`
}

/** Extract a file extension for icon selection. */
export function fileExtension(name: string | null | undefined): string {
	if (!name) return ''
	const i = name.lastIndexOf('.')
	return i > 0 ? name.slice(i + 1).toLowerCase() : ''
}

const DOC_EXT = new Set(['pdf', 'doc', 'docx', 'odt', 'txt', 'md', 'rtf', 'xls', 'xlsx', 'ppt', 'pptx'])
const IMAGE_EXT = new Set(['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg', 'heic', 'bmp', 'tiff'])
const ARCHIVE_EXT = new Set(['zip', 'tar', 'gz', 'bz2', 'xz', '7z', 'rar', 'zst'])
const MEDIA_EXT = new Set(['mp3', 'wav', 'flac', 'ogg', 'm4a', 'mp4', 'mkv', 'mov', 'webm'])
const CODE_EXT = new Set(['rs', 'ts', 'tsx', 'js', 'py', 'c', 'h', 'cpp', 'go', 'java', 'sh', 'json', 'toml', 'yaml'])

export type FileCategory = 'doc' | 'image' | 'archive' | 'media' | 'code' | 'generic'

export function fileCategory(name: string | null | undefined): FileCategory {
	const ext = fileExtension(name)
	if (DOC_EXT.has(ext)) return 'doc'
	if (IMAGE_EXT.has(ext)) return 'image'
	if (ARCHIVE_EXT.has(ext)) return 'archive'
	if (MEDIA_EXT.has(ext)) return 'media'
	if (CODE_EXT.has(ext)) return 'code'
	return 'generic'
}

/** Turn a device type into the label shown in the UI. */
export function deviceTypeLabel(type: string): string {
	switch (type) {
		case 'phone':
			return 'Phone'
		case 'tablet':
			return 'Tablet'
		case 'laptop':
			return 'Laptop'
		case 'tv':
			return 'TV'
		default:
			return 'Desktop'
	}
}

/** A stable colour accent for a device, derived from its id. */
export function deviceAccent(id: string): number {
	let hash = 0
	for (let i = 0; i < id.length; i++) hash = (hash * 31 + id.charCodeAt(i)) >>> 0
	return hash % 360
}

export function truncate(text: string, max = 60): string {
	if (text.length <= max) return text
	return `${text.slice(0, max - 1)}…`
}

export function pluralise(count: number, singular: string, plural?: string): string {
	return count === 1 ? singular : (plural ?? `${singular}s`)
}