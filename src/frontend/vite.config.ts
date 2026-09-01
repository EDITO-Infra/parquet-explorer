/**
 * Vite development/build configuration.
 *
 * In development, browser requests to /api are proxied to the Rust server so
 * the frontend can use the same relative API URLs as the deployed application.
 * MapLibre is excluded from Vite dependency pre-bundling because it ships its
 * own worker/module behavior.
 */
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  optimizeDeps: {
    exclude: ['maplibre-gl'],
  },
  server: {
    port: 5173,
    proxy: {
      '/api': {
        target: process.env.VITE_DEV_API_TARGET ?? 'http://localhost:8080',
        changeOrigin: true,
      },
    },
  },
})
