import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  server: {
    host: '0.0.0.0',
    port: 5173,
    proxy: {
      '/api': {
        target: 'http://127.0.0.1:5100',
        changeOrigin: true,
      },
      '/v1': {
        target: 'http://127.0.0.1:5100',
        changeOrigin: true,
      },
      '/ping': {
        target: 'http://127.0.0.1:5100',
        changeOrigin: true,
      },
    },
  },
})
