import { defineConfig } from 'eslint/config';
import js from '@eslint/js';
import tseslint from 'typescript-eslint';
import reactHooks from 'eslint-plugin-react-hooks';

export default defineConfig(
  { ignores: ['out/**', 'dist/**', 'node_modules/**', 'spikes/**', 'test-results/**', 'playwright-report/**'] },
  js.configs.recommended,
  tseslint.configs.recommended,
  {
    files: ['scripts/**/*.mjs', 'test/**/*.mjs', 'screenshots/**/*.mjs'],
    languageOptions: {
      globals: {
        Buffer: 'readonly',
        console: 'readonly',
        process: 'readonly',
        fetch: 'readonly',
        setTimeout: 'readonly',
        clearTimeout: 'readonly',
        setInterval: 'readonly',
      },
    },
  },
  {
    files: ['src/renderer/**/*.{ts,tsx}'],
    plugins: { 'react-hooks': reactHooks },
    rules: reactHooks.configs.recommended.rules,
  },
  {
    rules: {
      '@typescript-eslint/no-unused-vars': ['error', { argsIgnorePattern: '^_', varsIgnorePattern: '^_' }],
    },
  },
);
