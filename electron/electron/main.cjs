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

const { app, BrowserWindow, ipcMain, screen, shell, clipboard, globalShortcut } = require('electron');
const path = require('path');
const { execFile, exec } = require('child_process');
const net = require('net');
const fs = require('fs');
const os = require('os');

// Path of the prebuilt Rust backend, installed by scripts/install-user.sh.
const BINARY_PATH = path.join(os.homedir(), '.local', 'bin', 'spotlight-files');
// Unix socket the Rust daemon writes "toggle"/"show"/"hide" to.
const SOCKET_PATH = '/tmp/spotlight-files.sock';
// `--dev` flag switches the window source from the built bundle to the Vite dev server.
const IS_DEV = process.argv.includes('--dev');

// Disable GPU acceleration — XWayland GPU init often fails inside Electron;
// the transparent Spotlight window renders fine on the software compositor.
app.disableHardwareAcceleration();
app.commandLine.appendSwitch('no-sandbox');     // sandbox needs root-owned chrome-sandbox
app.commandLine.appendSwitch('disable-gpu');    // enforce software rendering

let win = null;   // the single Spotlight window (created lazily)

// Build the transparent, frameless window. Hidden until toggled.
function createWindow() {
  const primary = screen.getPrimaryDisplay();
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

  // Click-outside / focus-loss dismisses the window (macOS Spotlight behavior).
  win.on('blur', () => {
    if (win && win.isVisible()) {
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
  // Horizontal center; vertically ~12% above center (macOS Spotlight placement).
  const x = Math.round(primary.bounds.x + (primary.bounds.width - winWidth) / 2);
  const y = Math.round(primary.bounds.y + (primary.bounds.height - winHeight) / 2 - primary.bounds.height * 0.12);
  win.setBounds({ x, y, width: winWidth, height: winHeight });
  win.webContents.send('spotlight-show'); // tell renderer to clear + focus input
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

// ── IPC handlers (called by the renderer via preload.cjs) ──────────────

// Run a search: shell out to the Rust binary's `--search` JSON mode.
ipcMain.handle('search', async (_event, query) => {
  return new Promise((resolve, reject) => {
    execFile(BINARY_PATH, ['--search', query], (err, stdout, stderr) => {
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
      exec(`gtk-launch ${JSON.stringify(actionData)}`); // actionData = .desktop id
      break;
    case 'open_file':
      exec(`xdg-open ${JSON.stringify(actionData)}`);    // actionData = file path
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
  try { fs.unlinkSync(SOCKET_PATH); } catch (e) {}
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
    try { fs.unlinkSync(SOCKET_PATH); } catch (e) {}
    globalShortcut.unregisterAll();
  } catch (e) {}
});
