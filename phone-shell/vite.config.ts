import { defineConfig } from 'vite';

export default defineConfig({
  root: 'ui',
  server: {
    port: 5174,
    strictPort: true,
    // The shell imports the shared design tokens from the desktop app.
    fs: { allow: ['../..', '.'] },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
  },
});
