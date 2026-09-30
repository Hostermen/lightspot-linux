// eslint.config.js — Flat ESLint config for the Electron renderer + main.
//
// Minimal, modern config: parses .js/.jsx, applies the recommended JS
// ruleset, and defines the browser + node + commonjs globals the
// renderer/preload/main actually use. No style rules (no semicolons,
// no quotes) — formatting is left to the author.
//
// Run:  npx eslint .            (check)   npx eslint . --fix   (autofix)

import js from '@eslint/js';
import globals from 'globals';

export default [
  js.configs.recommended,
  {
    // Global rules applied to every file.
    rules: {
      // `_`-prefixed args are intentionally unused (e.g. `_event` in ipcMain).
      'no-unused-vars': ['error', { argsIgnorePattern: '^_' }],
    },
  },
  {
    // Renderer + preload: browser globals + the electronAPI bridge.
    files: ['src/**/*.{js,jsx}', 'electron/preload.cjs'],
    languageOptions: {
      globals: {
        ...globals.browser,
        electronAPI: 'readonly',
      },
      parserOptions: {
        ecmaFeatures: { jsx: true },
      },
      ecmaVersion: 2022,
      sourceType: 'module',
    },
  },
  {
    // Electron main process: Node + CommonJS.
    files: ['electron/main.cjs', 'electron/**/*.cjs'],
    languageOptions: {
      globals: {
        ...globals.node,
        ...globals.commonjs,
      },
      ecmaVersion: 2022,
      sourceType: 'commonjs',
    },
  },
  {
    // Vite config + scripts.
    files: ['*.config.js', 'scripts/**/*.js'],
    languageOptions: {
      globals: { ...globals.node, ...globals.commonjs },
      ecmaVersion: 2022,
      sourceType: 'module',
    },
  },
  {
    ignores: ['dist/**', 'node_modules/**'],
  },
];
