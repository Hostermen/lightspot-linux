// App.jsx — The Spotlight UI component (renderer side).
//
// A single React component that renders the search bar and results list,
// handles keyboard navigation, and talks to the main process through the
// `window.electronAPI` bridge exposed by preload.cjs.
//
// Data flow:
//   user types → debounce 150ms → electronAPI.search(query) → JSON results
//   → render rows → user navigates with arrows → Enter → electronAPI.activate(...)

import React, { useState, useEffect, useRef, useCallback } from 'react';
import './App.css';

function App() {
  // Current text in the search field.
  const [query, setQuery] = useState('');
  // Array of result items returned by the backend (title, subtitle, icon, action_*).
  const [results, setResults] = useState([]);
  // Index of the currently highlighted row (-1 = none / focus in search field).
  const [selected, setSelected] = useState(-1);
  // True while a search is in flight (could be used for a spinner).
  const [loading, setLoading] = useState(false);
  // Direct DOM ref to the input so we can focus it programmatically.
  const inputRef = useRef(null);
  // Ref to the results container so we can scroll the selected row into view.
  const listRef = useRef(null);
  // Holds the pending debounce timer id so we can cancel and replace it.
  const debounceRef = useRef(null);

  // On mount: focus the input and subscribe to "window shown" events so we can
  // clear + refocus every time the Electron window is toggled visible.
  useEffect(() => {
    inputRef.current?.focus();
    window.electronAPI?.onShow(() => {
      setQuery('');          // clear any previous query
      setResults([]);        // clear results
      setSelected(-1);       // reset selection
      setTimeout(() => inputRef.current?.focus(), 50); // refocus after re-render
    });
  }, []);

  // Debounced search effect — fires whenever `query` changes.
  useEffect(() => {
    if (debounceRef.current) clearTimeout(debounceRef.current); // cancel pending search
    // Empty query → no results.
    if (query.trim().length === 0) {
      setResults([]);
      setSelected(-1);
      return;
    }
    setLoading(true);
    // Wait 150ms after the last keystroke before searching (avoid keystroke spam).
    debounceRef.current = setTimeout(async () => {
      try {
        const json = await window.electronAPI.search(query); // IPC → Rust backend
        setResults(json.items || []);   // backend returns { items: [...] }
        setSelected(-1);                 // no row selected yet
      } catch (e) {
        console.error('search error', e);
      } finally {
        setLoading(false);
      }
    }, 150);
    // Cleanup: cancel the timer if the query changes before it fires.
    return () => clearTimeout(debounceRef.current);
  }, [query]);

  // Activate a result item: tell the main process to launch/open/copy, then hide.
  const activate = useCallback((item) => {
    if (!item) return;
    window.electronAPI.activate(item.action_type, item.action_data);
  }, []);

  // Global key handler for the spotlight (Esc, arrows, Enter, Tab).
  const handleKey = (e) => {
    if (e.key === 'Escape') {
      window.electronAPI.hide();          // Esc → dismiss
      return;
    }
    if (e.key === 'ArrowDown' || e.key === 'Tab') {
      e.preventDefault();                 // don't move focus out of the input
      if (results.length > 0) {
        // Wrap around to the top after the last result.
        setSelected((prev) => (prev + 1) % results.length);
      }
      return;
    }
    if (e.key === 'ArrowUp') {
      e.preventDefault();
      // Wrap around to the bottom when pressing Up from the top.
      setSelected((prev) => (prev <= 0 ? results.length - 1 : prev - 1));
      return;
    }
    if (e.key === 'Enter') {
      e.preventDefault();
      // Activate the selected row, or the first row if nothing is selected.
      if (selected >= 0 && selected < results.length) {
        activate(results[selected]);
      } else if (results.length > 0) {
        activate(results[0]);
      }
      return;
    }
  };

  // Scroll the selected row into view whenever the selection changes.
  useEffect(() => {
    if (selected >= 0 && listRef.current) {
      const el = listRef.current.children[selected]; // nth child = nth row
      if (el) el.scrollIntoView({ block: 'nearest' });
    }
  }, [selected]);

  // Auto-resize the Electron window to fit the rendered content.
  // Uses getBoundingClientRect().height (includes the border) so the bottom
  // outline is not clipped by the transparent window.
  useEffect(() => {
    const el = document.querySelector('.spotlight');
    if (el) {
      const h = Math.ceil(el.getBoundingClientRect().height);
      window.electronAPI?.resize(h);
    }
  }, [results]);

  return (
    <div className="spotlight" onKeyDown={handleKey}>
      {/* Search bar: magnifier icon + text input */}
      <div className="search-bar">
        <svg className="magnifier" width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <circle cx="11" cy="11" r="8" />
          <line x1="21" y1="21" x2="16.65" y2="16.65" />
        </svg>
        <input
          ref={inputRef}
          className="search-input"
          type="text"
          placeholder="Search apps, files, or calculate…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          spellCheck="false"
          autoComplete="off"
        />
      </div>
      {/* Results list — only rendered when there are results */}
      {results.length > 0 && (
        <>
          <div className="separator" />
          <div className="results" ref={listRef}>
            {results.map((item, i) => (
              <div
                key={i}
                className={`result-row ${i === selected ? 'selected' : ''}`}
                onMouseEnter={() => setSelected(i)}      // hover highlights
                onClick={() => activate(item)}            // click activates
              >
                {/* Icon — an emoji placeholder based on the result type */}
                <div className="result-icon">
                  {item.icon === 'accessories-calculator' ? (
                    <span className="icon-emoji">🧮</span>           // calculator result
                  ) : item.action_type === 'launch_app' ? (
                    <span className="icon-emoji">📦</span>           // application
                  ) : (
                    <span className="icon-emoji">📄</span>          // file
                  )}
                </div>
                {/* Title + subtitle (directory or "Application") */}
                <div className="result-text">
                  <div className="result-title">{item.title}</div>
                  <div className="result-subtitle">{item.subtitle}</div>
                </div>
              </div>
            ))}
          </div>
        </>
      )}
    </div>
  );
}

export default App;
