import react from '@vitejs/plugin-react';
import { defineConfig } from 'vitest/config';

const apiPort = process.env.ELESDEE_API_PORT || '4319';

export default defineConfig({
  plugins: [react()],
  optimizeDeps: {
    include: [
      '@tauri-apps/api/core',
      '@tauri-apps/api/event',
      '@tauri-apps/api/window'
    ]
  },
  build: {
    rollupOptions: {
      input: {
        main: 'index.html',
        phone: 'phone.html'
      }
    }
  },
  server: {
    port: 5173,
    strictPort: true,
    hmr: process.env.ELESDEE_API_PORT ? false : undefined,
    proxy: {
      '/api': {
        target: `http://127.0.0.1:${apiPort}`,
        changeOrigin: false,
        configure: process.env.ELESDEE_API_PORT
          ? (proxy) => {
              // The local core checks the browser origin. The test proxy
              // forwards local UI requests without it, while the core still
              // checks the host.
              proxy.on('proxyReq', (proxyRequest) => {
                proxyRequest.removeHeader('origin');
              });
            }
          : undefined,
      },
    },
  },
  test: {
    environment: 'jsdom',
    setupFiles: './src/test/setup.ts',
    globals: true,
    restoreMocks: true,
  },
});
