import React, { useState, useEffect, useRef, useCallback } from 'react';
import './App.css';

function App() {
  const [query, setQuery] = useState('');
  const [results, setResults] = useState([]);
  const [selected, setSelected] = useState(-1);
  const [loading, setLoading] = useState(false);
  const inputRef = useRef(null);
  const listRef = useRef(null);
  const debounceRef = useRef(null);

  // Focus input on mount and when window becomes visible
  useEffect(() => {
    inputRef.current?.focus();
    window.electronAPI?.onShow(() => {
      setQuery('');
      setResults([]);
      setSelected(-1);
      setTimeout(() => inputRef.current?.focus(), 50);
    });
  }, []);

  // Debounced search
  useEffect(() => {
    if (debounceRef.current) clearTimeout(debounceRef.current);
    if (query.trim().length === 0) {
      setResults([]);
      setSelected(-1);
      return;
    }
    setLoading(true);
    debounceRef.current = setTimeout(async () => {
      try {
        const json = await window.electronAPI.search(query);
        setResults(json.items || []);
        setSelected(-1);
      } catch (e) {
        console.error('search error', e);
      } finally {
        setLoading(false);
      }
    }, 150);
    return () => clearTimeout(debounceRef.current);
  }, [query]);

  const activate = useCallback((item) => {
    if (!item) return;
    window.electronAPI.activate(item.action_type, item.action_data);
  }, []);

  const handleKey = (e) => {
    if (e.key === 'Escape') {
      window.electronAPI.hide();
      return;
    }
    if (e.key === 'ArrowDown' || e.key === 'Tab') {
      e.preventDefault();
      if (results.length > 0) {
        setSelected((prev) => (prev + 1) % results.length);
      }
      return;
    }
    if (e.key === 'ArrowUp') {
      e.preventDefault();
      setSelected((prev) => (prev <= 0 ? results.length - 1 : prev - 1));
      return;
    }
    if (e.key === 'Enter') {
      e.preventDefault();
      if (selected >= 0 && selected < results.length) {
        activate(results[selected]);
      } else if (results.length > 0) {
        activate(results[0]);
      }
      return;
    }
  };

  // Scroll selected into view
  useEffect(() => {
    if (selected >= 0 && listRef.current) {
      const el = listRef.current.children[selected];
      if (el) el.scrollIntoView({ block: 'nearest' });
    }
  }, [selected]);

  // Auto-resize window based on content
  useEffect(() => {
    const el = document.querySelector('.spotlight');
    if (el) {
      const h = Math.ceil(el.getBoundingClientRect().height);
      window.electronAPI?.resize(h);
    }
  }, [results]);

  return (
    <div className="spotlight" onKeyDown={handleKey}>
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
      {results.length > 0 && (
        <>
          <div className="separator" />
          <div className="results" ref={listRef}>
            {results.map((item, i) => (
              <div
                key={i}
                className={`result-row ${i === selected ? 'selected' : ''}`}
                onMouseEnter={() => setSelected(i)}
                onClick={() => activate(item)}
              >
                <div className="result-icon">
                  {item.icon === 'accessories-calculator' ? (
                    <span className="icon-emoji">🧮</span>
                  ) : item.action_type === 'launch_app' ? (
                    <span className="icon-emoji">📦</span>
                  ) : (
                    <span className="icon-emoji">📄</span>
                  )}
                </div>
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
