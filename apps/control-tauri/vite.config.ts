import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import path from 'path';
import {defineConfig} from 'vite';

export default defineConfig(() => {
  return {
    plugins: [react(), tailwindcss()],
    resolve: {
      alias: {
        '@': path.resolve(__dirname, '.'),
        '@winfaceunlock/control-client': path.resolve(
          __dirname,
          '../../packages/control-client/src',
        ),
        '@winfaceunlock/control-tauri-transport': path.resolve(
          __dirname,
          '../../packages/control-tauri-transport/src',
        ),
        '@tauri-apps/api': path.resolve(__dirname, 'node_modules/@tauri-apps/api'),
      },
    },
    server: {
      host: '127.0.0.1',
      port: 3000,
      strictPort: true,
      // HMR is disabled in AI Studio via DISABLE_HMR env var.
      // File watching is disabled when requested to prevent flickering during agent edits.
      hmr: process.env.DISABLE_HMR !== 'true',
      watch: process.env.DISABLE_HMR === 'true' ? null : {},
    },
  };
});
