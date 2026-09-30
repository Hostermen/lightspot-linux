// App.jsx — The Spotlight UI component (renderer side).
//
// A single React component that renders the search bar and results list,
// handles keyboard navigation, and talks to the main process through the
// `window.electronAPI` bridge exposed by preload.cjs.
//
// Data flow:
//   user types → input updates URGENTLY (useDeferredValue keeps it snappy)
//   → 80ms debounce → electronAPI.search(deferredQuery) → JSON results
//   → startTransition(setResults) → render rows (interruptible)
//   → user navigates with arrows → Enter → electronAPI.activate(...)

import { useState, useEffect, useRef, useCallback, useDeferredValue, startTransition } from 'react';
import './App.css';

function App() {
  // Current text in the search field — updates URGENTLY on every keystroke
  // so letters appear immediately. The search uses `deferredQuery` (below)
  // which React updates at lower priority, keeping the input responsive.
  const [query, setQuery] = useState('');
  // Deferred copy of `query`: React updates this when the renderer has spare
  // time. The search effect depends on this, so searches and the resulting
  // results/icons/resize cascade never block input painting.
  const deferredQuery = useDeferredValue(query);
  // Array of result items returned by the backend (title, subtitle, icon, action_*).
  const [results, setResults] = useState([]);
  // Index of the currently highlighted row (-1 = none / focus in search field).
  const [selected, setSelected] = useState(-1);
  // Direct DOM ref to the input so we can focus it programmatically.
  const inputRef = useRef(null);
  // Ref to the results container so we can scroll the selected row into view.
  const listRef = useRef(null);
  // Holds the pending debounce timer id so we can cancel and replace it.
  const debounceRef = useRef(null);
  // Map: freedesktop icon name → data URL (or null). Populated after each search.
  const [iconMap, setIconMap] = useState({});

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

  // Debounced search effect — fires whenever `deferredQuery` changes (NOT
  // `query`), so the input never blocks on search work.
  useEffect(() => {
    if (debounceRef.current) clearTimeout(debounceRef.current);
    // Empty query → no results.
    if (deferredQuery.trim().length === 0) {
      setResults([]);
      setSelected(-1);
      return;
    }
    // Wait 80ms after the last keystroke before searching.
    debounceRef.current = setTimeout(async () => {
      try {
        const json = await window.electronAPI.search(deferredQuery); // IPC → backend
        // Mark results as a LOW-PRIORITY transition: if the user types
        // again before the results list finishes rendering, React
        // interrupts this render and prioritizes the input instead.
        startTransition(() => {
          setResults(json.items || []);   // backend returns { items: [...] }
          setSelected(-1);                 // no row selected yet
        });
      } catch (e) {
        console.error('search error', e);
      }
    }, 80);
    // Cleanup: cancel the timer if the query changes before it fires.
    return () => clearTimeout(debounceRef.current);
  }, [deferredQuery]);

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

  // Resolve freedesktop icon names to system icons whenever results change.
  useEffect(() => {
    const names = results.map((r) => r.icon).filter(Boolean);
    if (names.length === 0) { setIconMap({}); return; }
    window.electronAPI?.getIcons([...new Set(names)]).then(setIconMap);
  }, [results]);

  // Auto-resize the Electron window to fit the rendered content.
  // Debounced so rapid result changes (e.g. fast typing) don't trigger
  // a window resize on every intermediate state — only the final one.
  useEffect(() => {
    const t = setTimeout(() => {
      const el = document.querySelector('.spotlight');
      if (el) {
        const h = Math.ceil(el.getBoundingClientRect().height);
        window.electronAPI?.resize(h);
      }
    }, 60);
    return () => clearTimeout(t);
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
                {/* Icon — system icon resolved via GTK3, or fallback */}
                <div className="result-icon">
                  {iconMap[item.icon] ? (
                    <img src={iconMap[item.icon]} alt="" className="icon-img" />
                  ) : item.icon === 'accessories-calculator' ? (
                    <svg className="icon-fallback" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                      <rect x="4" y="2" width="16" height="20" rx="2" />
                      <line x1="8" y1="6" x2="16" y2="6" />
                      <line x1="8" y1="10" x2="8" y2="10" />
                      <line x1="12" y1="10" x2="12" y2="10" />
                      <line x1="16" y1="10" x2="16" y2="10" />
                      <line x1="8" y1="14" x2="8" y2="14" />
                      <line x1="12" y1="14" x2="12" y2="14" />
                      <line x1="16" y1="14" x2="16" y2="18" />
                      <line x1="8" y1="18" x2="8" y2="18" />
                      <line x1="12" y1="18" x2="12" y2="18" />
                    </svg>
                  ) : (
                    <svg className="icon-fallback" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                      <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
                      <polyline points="14 2 14 8 20 8" />
                    </svg>
                  )}
                  {item.is_content && (
                    <span className="content-badge" title="Content match">
                      <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round">
                        <circle cx="11" cy="11" r="8" />
                        <line x1="21" y1="21" x2="16.65" y2="16.65" />
                      </svg>
                    </span>
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
