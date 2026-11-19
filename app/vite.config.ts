import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// The panel's web UI. Tauri serves ui/dist in builds and this dev server while developing.
export default defineConfig({
  root: 'ui',
  plugins: [svelte()],
  clearScreen: false,
  // The fixtures and the share picture read the repository's logos (assets/), outside the UI's own folder.
  server: { port: 1420, strictPort: true, fs: { allow: ['..', '../..'] } },
  build: { outDir: 'dist', emptyOutDir: true, target: 'safari16' },
});
