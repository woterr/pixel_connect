import vue from '@vitejs/plugin-vue'
import { resolve } from 'path'
import AutoImport from 'unplugin-auto-import/vite'
import { defineConfig } from 'vitest/config'

// Pixel Connect front end.
// The build is embedded by Tauri into src-tauri/dist; see src-tauri/tauri.conf.json.
export default defineConfig({
	plugins: [
		vue(),
		AutoImport({
			imports: ['vue'],
			dts: './src/auto-imports.d.ts',
		}),
	],
	clearScreen: false,
	envPrefix: ['VITE_', 'TAURI_'],
	server: {
		port: 1420,
		strictPort: true,
		fs: {
			allow: [resolve(__dirname)],
		},
	},
	build: {
		outDir: './dist',
		target: process.env.TAURI_PLATFORM == 'windows' ? 'chrome105' : 'safari15',
		minify: !process.env.TAURI_DEBUG ? 'esbuild' : false,
		sourcemap: !!process.env.TAURI_DEBUG,
		emptyOutDir: true,
	},
	test: {
		include: ['tests/unit/**/*.{test,spec}.{js,mjs,cjs,ts,mts,cts,jsx,tsx}'],
		environment: 'node',
	},
})