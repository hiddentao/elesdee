import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

// Builds the phone layout into a self-contained package. The packaging script
// adds the manifest and the archive after this build.
export default defineConfig({
  plugins: [react()],
  base: './',
  build: {
    outDir: 'dist-phone',
    emptyOutDir: true,
    rollupOptions: {
      input: {
        phone: 'phone.html',
      },
    },
  },
});
