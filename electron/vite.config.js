// vite.config.js — Vite build configuration for the React frontend.
//
// Vite bundles src/main.jsx and its imports into electron/dist/, which the
// Electron main process loads as the window content in production.

import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],         // enable JSX + React Fast Refresh
  base: './',                 // relative asset URLs — required for `loadFile()`
  build: {
    outDir: 'dist',            // output directory (loaded by main.cjs in production)
    emptyOutDir: true,         // clean dist/ before each build
  },
  server: {
    port: 5173,                // dev server port (used with --dev flag)
  },
});
