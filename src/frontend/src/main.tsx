/**
 * Browser entry point.
 *
 * This file intentionally does very little: it loads global styles, creates the
 * React root, and mounts <App />. Application workflows belong in App.tsx and
 * protocol/data-format details belong in plain TypeScript helpers.
 */
import React from 'react'
import ReactDOM from 'react-dom/client'
import 'maplibre-gl/dist/maplibre-gl.css'
import './styles.css'
import App from './App'

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode><App /></React.StrictMode>,
)
