import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { phoneFixtures } from './ui/fixtures/phone-api.mjs';

// The panel's web UI. Tauri serves ui/dist in builds and this dev server while developing (with /api/ answered from
// the fixtures, for 在手机上看's page).
export default defineConfig({
  root: 'ui',
  // The Svelte config sits beside this file, one level above the root; the path is relative to the root.
  plugins: [svelte({ configFile: '../svelte.config.js' }), phoneFixtures()],
  clearScreen: false,
  // The fixtures and the share picture read the repository's logos (assets/), outside the UI's own folder.
  server: { port: 1420, strictPort: true, fs: { allow: ['..', '../..'] } },
  build: { outDir: 'dist', emptyOutDir: true, target: 'safari16' },
});
