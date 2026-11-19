import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// The panel's web UI. Tauri serves ui/dist in builds and this dev server while developing.
export default defineConfig({
  root: 'ui',
  // The Svelte config sits beside this file, one level above the root; the path is relative to the root.
  plugins: [svelte({ configFile: '../svelte.config.js' })],
  clearScreen: false,
  // The fixtures and the share picture read the repository's logos (assets/), outside the UI's own folder.
  server: { port: 1420, strictPort: true, fs: { allow: ['..', '../..'] } },
  build: { outDir: 'dist', emptyOutDir: true, target: 'safari16' },
});
