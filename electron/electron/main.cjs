// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Hostermen

// main.cjs — Electron main process.
//
// Responsibilities:
//   • Create and manage the transparent, frameless Spotlight window.
//   • Listen on a Unix socket (/tmp/spotlight-files.sock) for "toggle" /
//     "show" / "hide" commands coming from the Rust hotkey daemon.
//   • Handle IPC from the renderer (React): run a search via the Rust
//     binary's `--search` mode, activate results (launch app / open file /
//     copy to clipboard), and resize the window to fit the results.
//   • Register Super+Space as a fallback hotkey.
//
// Uses CommonJS (.cjs) because Electron's main entry works best with CJS,
// while Vite bundles the renderer as ESM.

const { app, BrowserWindow, ipcMain, screen, clipboard, globalShortcut } = require('electron');
const path = require('path');
const { execFile, spawn } = require('child_process');
const net = require('net');
const readline = require('readline');
const fs = require('fs');
const os = require('os');

// Path of the prebuilt Rust backend, installed by scripts/install-user.sh.
const BINARY_PATH = path.join(os.homedir(), '.local', 'bin', 'spotlight-files');
// Unix socket the Rust daemon writes "toggle"/"show"/"hide" to.
const SOCKET_PATH = '/tmp/spotlight-files.sock';
// Unix socket the Rust daemon serves search JSON over. The Electron app
// connects here per query instead of spawning a `--search` subprocess,
// which keeps the backend's app list + Tantivy index warm in memory.
const SEARCH_SOCKET = '/tmp/spotlight-search.sock';
// `--dev` flag switches the window source from the built bundle to the Vite dev server.
const IS_DEV = process.argv.includes('--dev');

// SwiftShader software GL: keeps the GPU process alive (needed for
// compositing + alpha channel → transparent window) but avoids the
// GPU driver crash (error 1002). Unlike app.disableHardwareAcceleration()
// which kills ALL compositing and makes the window opaque, SwiftShader
// runs a software GL implementation so the compositor stays functional.
// See: open-webui/desktop#178, #273
//
// Electron 31 enables Vulkan by default on Linux, so the GPU process
// still loads the host Vulkan ICD (Intel ANV / NVIDIA) even with the
// ANGLE+SwiftShader GL switches below. Under startup CPU/disk load the
// Vulkan init races and fails with error 1002, which dumps core
// (status=5/TRAP). `disable-vulkan` forces the SwiftShader GL path for
// compositing too, and `disable-gpu-sandbox` makes the GPU process
// launch reliably under the systemd user unit.
app.commandLine.appendSwitch('no-sandbox');
app.commandLine.appendSwitch('disable-gpu-sandbox');
app.commandLine.appendSwitch('disable-vulkan');
app.commandLine.appendSwitch('use-gl', 'angle');
app.commandLine.appendSwitch('use-angle', 'swiftshader');
app.commandLine.appendSwitch('ozone-platform=wayland'); // native Wayland for alpha

let win = null;   // the single Spotlight window (created lazily)
let showTime = 0;  // timestamp of last show() — used to ignore spurious blur on Wayland

// Build the transparent, frameless window. Hidden until toggled.
function createWindow() {
  const winWidth = 680;          // fixed width, matches the CSS .spotlight width
  const winMaxHeight = 500;      // cap so a long results list can't overflow

  win = new BrowserWindow({
    width: winWidth,
    height: winMaxHeight,
    minWidth: winWidth,
    maxWidth: winWidth,
    minHeight: 80,               // just the search bar when no results
    maxHeight: winMaxHeight,
    frame: false,                // no OS titlebar/borders — we draw our own rounded card
    transparent: true,           // allows the rgba background + blur to show the desktop
    resizable: false,
    show: false,                 // start hidden; shown on first toggle
    skipTaskbar: true,           // don't appear in the taskbar/dock
    alwaysOnTop: true,           // float above other windows
    hasShadow: false,            // we render our own drop shadow via CSS
    backgroundColor: '#00000000', // fully transparent base
    webPreferences: {
      preload: path.join(__dirname, 'preload.cjs'), // bridge to renderer
      contextIsolation: true,    // isolate renderer from Node — security best practice
      nodeIntegration: false,    // renderer has no direct Node access
    },
  });

  if (IS_DEV) {
    // Dev mode: load from the Vite dev server and open DevTools.
    win.loadURL('http://localhost:5173');
    win.webContents.openDevTools({ mode: 'detach' });
  } else {
    // Production: load the Vite-built bundle from electron/dist/.
    win.loadFile(path.join(__dirname, '..', 'dist', 'index.html'));
  }

  // Click-outside / focus-loss dismisses the window.
  // On Wayland, blur can fire spuriously right after show, so we add a small
  // grace period — ignore blur events within 500ms of showing.
  win.on('blur', () => {
    if (win && win.isVisible() && Date.now() - showTime > 500) {
      win.hide();
    }
  });

  // Drop our reference when the window is closed.
  win.on('closed', () => {
    win = null;
  });
}

// Show the window, recentered, with just the search-bar height initially.
// The renderer grows the window once results arrive via the 'resize' IPC.
function showWindow() {
  if (!win) createWindow();
  const primary = screen.getPrimaryDisplay();
  const winWidth = 680;
  const winHeight = 80;          // initial height — grows with results
  // Horizontal center; vertically ~12% above center.
  const x = Math.round(primary.bounds.x + (primary.bounds.width - winWidth) / 2);
  const y = Math.round(primary.bounds.y + (primary.bounds.height - winHeight) / 2 - primary.bounds.height * 0.12);
  win.setBounds({ x, y, width: winWidth, height: winHeight });
  win.webContents.send('spotlight-show'); // tell renderer to clear + focus input
  showTime = Date.now();
  win.show();
  win.focus();
}

// Hide the window (it stays alive in the background).
function hideWindow() {
  if (win) win.hide();
}

// Toggle visibility — called from the socket listener and the Super+Space shortcut.
function toggleWindow() {
  if (win && win.isVisible()) hideWindow();
  else showWindow();
}

// ── Icon resolution (Python3 + GTK3) ───────────────────────────────────

// Cache: freedesktop icon name → data URL (or null if not found).
// Persisted across searches so we only call Python3 for new icon names.
const iconCache = {};

// Python3 script run as a PERSISTENT child process. It imports GTK once
// (the expensive part, ~400ms), then enters a request loop: it reads icon
// names from stdin (one per line, terminated by a blank line) and writes
// `name\tmime\tdata` lines to stdout. Keeping the process alive means
// every icon batch after the first skips the GTK import and resolves in
// just a few milliseconds.
const ICON_PY_SERVER = [
  'import gi, sys, base64',
  'gi.require_version("Gtk", "3.0")',
  'from gi.repository import Gtk',
  'theme = Gtk.IconTheme.get_default()',
  'def lookup(name):',
  '    icon = theme.lookup_icon(name, 48, 0)',
  '    if icon: return icon',
  '    for prefix, fallback in [("text-", "text-x-generic"), ("application-", "application-x-generic"), ("image-", "image-x-generic"), ("audio-", "audio-x-generic"), ("video-", "video-x-generic")]:',
  '        if name.startswith(prefix) and name != fallback:',
  '            return theme.lookup_icon(fallback, 48, 0)',
  '    return theme.lookup_icon("text-x-generic", 48, 0)',
  'while True:',
  '    line = sys.stdin.readline()',
  '    if not line: break',
  '    name = line.strip()',
  '    if not name: continue',
  '    icon = lookup(name)',
  '    if icon:',
  '        path = icon.get_filename()',
  '        try:',
  '            with open(path, "rb") as f:',
  '                data = base64.b64encode(f.read()).decode()',
  '                ext = path.rsplit(".", 1)[-1].lower()',
  '                mime = "image/svg+xml" if ext == "svg" else "image/" + ext',
  '                sys.stdout.write(name + "\\t" + mime + "\\t" + data + "\\n")',
  '        except Exception:',
  '            sys.stdout.write(name + "\\tNOTFOUND\\t\\n")',
  '    else:',
  '        sys.stdout.write(name + "\\tNOTFOUND\\t\\n")',
  '    sys.stdout.flush()',
].join('\n');

// The persistent Python process + its stdout line reader. Lazily spawned
// on first icon request so apps that never show icons never pay the cost.
let iconProc = null;
let iconProcStdout = null;
// Queue of pending icon-batch requests: each entry holds the names being
// resolved and the promise resolver. Responses arrive in order, so the
// head of the queue is always the one receiving lines.
const iconQueue = [];

function ensureIconProc() {
  if (iconProc) return;
  iconProc = spawn('python3', ['-c', ICON_PY_SERVER], { stdio: ['pipe', 'pipe', 'ignore'] });
  iconProcStdout = readline.createInterface({ input: iconProc.stdout });
  iconProcStdout.on('line', (line) => {
    if (iconQueue.length === 0) return;
    const pending = iconQueue[0];
    const [name, mime, data] = line.split('\t');
    if (name) {
      if (mime === 'NOTFOUND' || !data) {
        iconCache[name] = null;
      } else {
        iconCache[name] = `data:${mime};base64,${data}`;
      }
      pending.received++;
    }
    // When the current batch has received all its responses, resolve it.
    if (pending.received >= pending.count) {
      iconQueue.shift();
      const result = {};
      for (const n of pending.names) result[n] = iconCache[n] ?? null;
      pending.resolve(result);
    }
  });
  iconProc.on('error', () => {
    // If the process can't start, fail all pending and mark for retry.
    while (iconQueue.length) {
      const p = iconQueue.shift();
      for (const n of p.names) iconCache[n] = null;
      p.resolve({});
    }
    iconProc = null;
    iconProcStdout = null;
  });
  iconProc.on('exit', () => {
    iconProc = null;
    iconProcStdout = null;
  });
}

// Resolve a list of freedesktop icon names to data URLs.
// Returns a map: { iconName: "data:image/png;base64,..." | null }.
function resolveIcons(iconNames) {
  return new Promise((resolve) => {
    const unique = [...new Set(iconNames)].filter(Boolean);
    const uncached = unique.filter((n) => !(n in iconCache));

    if (uncached.length === 0) {
      const result = {};
      for (const n of unique) result[n] = iconCache[n];
      resolve(result);
      return;
    }

    ensureIconProc();
    if (!iconProc) {
      // Process failed to start — degrade gracefully to nulls.
      for (const n of uncached) iconCache[n] = null;
      const result = {};
      for (const n of unique) result[n] = iconCache[n] ?? null;
      resolve(result);
      return;
    }

    // Enqueue this batch and write the names to the process's stdin.
    iconQueue.push({ names: uncached, count: uncached.length, received: 0, resolve });
    for (const n of uncached) iconProc.stdin.write(n + '\n');
  });
}

// ── IPC handlers (called by the renderer via preload.cjs) ──────────────

// Resolve freedesktop icon names to data URLs (called after search results arrive).
ipcMain.handle('get-icons', async (_event, iconNames) => {
  return resolveIcons(iconNames);
});

// Run a query against the daemon's warm search socket. Falls back to
// spawning the backend binary with `--search` if the socket isn't
// available (e.g. the daemon isn't up yet at app launch, or a very old
// install without the search server). The socket path is the query
// followed by a newline; the daemon replies with one JSON line.
function searchViaSocket(query) {
  return new Promise((resolve, reject) => {
    const sock = net.createConnection(SEARCH_SOCKET);
    let buf = '';
    const timer = setTimeout(() => sock.destroy(new Error('search socket timeout')), 3000);
    sock.on('data', (d) => { buf += d.toString(); });
    sock.on('end', () => { clearTimeout(timer); resolve(buf); });
    sock.on('error', (e) => { clearTimeout(timer); reject(e); });
    sock.on('connect', () => sock.write(query + '\n'));
  });
}

// Run a search. Tries the warm daemon socket first (fast path); on any
// failure (socket missing / daemon not running / parse error) it falls
// back to the one-shot `--search` subprocess so the app still works.
ipcMain.handle('search', async (_event, query) => {
  try {
    const out = await searchViaSocket(query);
    if (out && out.trim()) {
      return JSON.parse(out);
    }
  } catch {
    // Socket unavailable — fall through to the subprocess fallback.
  }
  return new Promise((resolve, reject) => {
    execFile(BINARY_PATH, ['--search', query], (err, stdout) => {
      if (err) {
        reject(err);                 // binary missing or crashed
        return;
      }
      try {
        resolve(JSON.parse(stdout)); // parse the JSON the backend produced
      } catch (e) {
        reject(e);                    // malformed JSON (shouldn't happen)
      }
    });
  });
});

// Activate a result based on its action_type.
ipcMain.handle('activate', async (_event, actionType, actionData) => {
  switch (actionType) {
    case 'launch_app':
      execFile('gtk-launch', [actionData]); // actionData = .desktop id
      break;
    case 'open_file':
      execFile('xdg-open', [actionData]);    // actionData = file path
      break;
    case 'copy':
      clipboard.writeText(actionData);                  // actionData = result text
      break;
  }
  hideWindow();   // always dismiss after activating a result
});

// Renderer requests to hide (Esc key).
ipcMain.on('hide', () => hideWindow());

// Renderer requests a height change to fit the results list.
ipcMain.handle('resize', (_event, height) => {
  if (win && win.isVisible()) {
    const bounds = win.getBounds();
    // Keep position and width; only change height, capped at 500.
    win.setBounds({ x: bounds.x, y: bounds.y, width: 680, height: Math.min(height, 500) });
  }
});

// ── Unix socket server (receives commands from the Rust daemon) ────────

function setupSocket() {
  // Remove a stale socket file from a previous crash.
  try { fs.unlinkSync(SOCKET_PATH); } catch { /* stale socket */ }
  // One connection at a time; the daemon opens, writes a command, and closes.
  const server = net.createServer((conn) => {
    let data = '';
    conn.on('data', (chunk) => { data += chunk; });   // buffer the message
    conn.on('end', () => {
      // Dispatch based on the command word the daemon sent.
      if (data.trim() === 'toggle') toggleWindow();
      else if (data.trim() === 'show') showWindow();
      else if (data.trim() === 'hide') hideWindow();
    });
  });
  server.listen(SOCKET_PATH, () => {
    fs.chmodSync(SOCKET_PATH, 0o600); // only the owner may talk to the socket
    console.log('Socket listening at', SOCKET_PATH);
  });
  server.on('error', (e) => console.error('Socket error:', e.message));
  return server;
}

let socketServer = null;

// ── App lifecycle ─────────────────────────────────────────────────────

app.whenReady().then(() => {
  createWindow();             // pre-create so the first toggle is instant
  socketServer = setupSocket();

  // Fallback hotkey: Super+Space (works even without evdev/input group).
  try {
    globalShortcut.register('Super+Space', () => toggleWindow());
  } catch (e) {
    console.error('Failed to register shortcut:', e.message);
  }
});

// Keep the app alive when all windows are closed (we want a persistent daemon).
app.on('window-all-closed', (e) => {
  e.preventDefault();
});

// Clean up the socket and shortcuts on quit.
app.on('before-quit', () => {
  try {
    if (socketServer) socketServer.close();
    try { fs.unlinkSync(SOCKET_PATH); } catch { /* stale socket */ }
    globalShortcut.unregisterAll();
  } catch { /* best-effort cleanup */ }
});
