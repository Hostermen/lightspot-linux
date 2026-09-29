// main.jsx — React entry point.
//
// Mounts the <App/> component into the #root div in index.html. Wrapped in
// <React.StrictMode> which surfaces potential problems in development
// (double-invocation of effects, deprecated APIs) without affecting production.

import React from 'react';
import { createRoot } from 'react-dom/client';
import App from './App.jsx';
import './index.css';

createRoot(document.getElementById('root')).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
