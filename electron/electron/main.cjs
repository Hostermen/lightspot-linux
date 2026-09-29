const { app, BrowserWindow, ipcMain, screen, shell, clipboard, globalShortcut } = require('electron');
const path = require('path');
const { execFile, exec } = require('child_process');
const net = require('net');
const fs = require('fs');
const os = require('os');

const BINARY_PATH = path.join(os.homedir(), '.local', 'bin', 'spotlight-files');
const SOCKET_PATH = '/tmp/spotlight-files.sock';
const IS_DEV = process.argv.includes('--dev');

// Disable GPU acceleration — XWayland GPU init often fails
app.disableHardwareAcceleration();
app.commandLine.appendSwitch('no-sandbox');
app.commandLine.appendSwitch('disable-gpu');

let win = null;

function createWindow() {
  const primary = screen.getPrimaryDisplay();
  const winWidth = 680;
  const winMaxHeight = 500;

  win = new BrowserWindow({
    width: winWidth,
    height: winMaxHeight,
    minWidth: winWidth,
    maxWidth: winWidth,
    minHeight: 80,
    maxHeight: winMaxHeight,
    frame: false,
    transparent: true,
    resizable: false,
    show: false,
    skipTaskbar: true,
    alwaysOnTop: true,
    hasShadow: false,
    backgroundColor: '#00000000',
    webPreferences: {
      preload: path.join(__dirname, 'preload.cjs'),
      contextIsolation: true,
      nodeIntegration: false,
    },
  });

  if (IS_DEV) {
    win.loadURL('http://localhost:5173');
    win.webContents.openDevTools({ mode: 'detach' });
  } else {
    win.loadFile(path.join(__dirname, '..', 'dist', 'index.html'));
  }

  win.on('blur', () => {
    if (win && win.isVisible()) {
      win.hide();
    }
  });

  win.on('closed', () => {
    win = null;
  });
}

function showWindow() {
  if (!win) createWindow();
  const primary = screen.getPrimaryDisplay();
  const winWidth = 680;
  const winHeight = 80; // initial — will grow with results
  const x = Math.round(primary.bounds.x + (primary.bounds.width - winWidth) / 2);
  const y = Math.round(primary.bounds.y + (primary.bounds.height - winHeight) / 2 - primary.bounds.height * 0.12);
  win.setBounds({ x, y, width: winWidth, height: winHeight });
  win.webContents.send('spotlight-show');
  win.show();
  win.focus();
}

function hideWindow() {
  if (win) win.hide();
}

function toggleWindow() {
  if (win && win.isVisible()) hideWindow();
  else showWindow();
}

// ── IPC ──────────────────────────────────────────────────────────

ipcMain.handle('search', async (_event, query) => {
  return new Promise((resolve, reject) => {
    execFile(BINARY_PATH, ['--search', query], (err, stdout, stderr) => {
      if (err) {
        reject(err);
        return;
      }
      try {
        resolve(JSON.parse(stdout));
      } catch (e) {
        reject(e);
      }
    });
  });
});

ipcMain.handle('activate', async (_event, actionType, actionData) => {
  switch (actionType) {
    case 'launch_app':
      exec(`gtk-launch ${JSON.stringify(actionData)}`);
      break;
    case 'open_file':
      exec(`xdg-open ${JSON.stringify(actionData)}`);
      break;
    case 'copy':
      clipboard.writeText(actionData);
      break;
  }
  hideWindow();
});

ipcMain.on('hide', () => hideWindow());

ipcMain.handle('resize', (_event, height) => {
  if (win && win.isVisible()) {
    const bounds = win.getBounds();
    win.setBounds({ x: bounds.x, y: bounds.y, width: 680, height: Math.min(height, 500) });
  }
});

// ── Unix socket for Rust daemon ───────────────────────────────────

function setupSocket() {
  try { fs.unlinkSync(SOCKET_PATH); } catch (e) {}
  const server = net.createServer((conn) => {
    let data = '';
    conn.on('data', (chunk) => { data += chunk; });
    conn.on('end', () => {
      if (data.trim() === 'toggle') toggleWindow();
      else if (data.trim() === 'show') showWindow();
      else if (data.trim() === 'hide') hideWindow();
    });
  });
  server.listen(SOCKET_PATH, () => {
    fs.chmodSync(SOCKET_PATH, 0o600);
    console.log('Socket listening at', SOCKET_PATH);
  });
  server.on('error', (e) => console.error('Socket error:', e.message));
  return server;
}

let socketServer = null;

// ── App lifecycle ─────────────────────────────────────────────────

app.whenReady().then(() => {
  createWindow();
  socketServer = setupSocket();

  // Fallback hotkey: Super+Space
  try {
    globalShortcut.register('Super+Space', () => toggleWindow());
  } catch (e) {
    console.error('Failed to register shortcut:', e.message);
  }
});

app.on('window-all-closed', (e) => {
  e.preventDefault();
});

app.on('before-quit', () => {
  try {
    if (socketServer) socketServer.close();
    try { fs.unlinkSync(SOCKET_PATH); } catch (e) {}
    globalShortcut.unregisterAll();
  } catch (e) {}
});
