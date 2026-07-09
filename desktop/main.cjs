const { app, BrowserWindow, ipcMain, protocol } = require('electron');
const fs = require('node:fs');
const path = require('node:path');
const { spawn } = require('node:child_process');

protocol.registerSchemesAsPrivileged([
  {
    scheme: 'rhwp',
    privileges: {
      standard: true,
      secure: true,
      supportFetchAPI: true,
      corsEnabled: true,
      stream: true,
    },
  },
]);

let mainWindow = null;
let mcp = null;
let nextMcpId = 1;
const mcpPending = new Map();
let smokeExitScheduled = false;

function studioRoot() {
  return app.isPackaged
    ? path.join(process.resourcesPath, 'studio')
    : path.resolve(__dirname, '..', 'rhwp-studio', 'dist');
}

function mcpBinaryPath() {
  const name = process.platform === 'win32' ? 'rhwp-mcp.exe' : 'rhwp-mcp';
  if (app.isPackaged) return path.join(process.resourcesPath, 'bin', name);
  const candidates = [
    path.resolve(__dirname, '..', 'target', 'desktop-release', name),
    path.resolve(__dirname, '..', 'target', 'release', name),
    path.resolve(__dirname, '..', 'target', 'debug', name),
  ];
  return candidates.find((candidate) => fs.existsSync(candidate)) || candidates[0];
}

function registerStudioProtocol() {
  protocol.handle('rhwp', async (request) => {
    const url = new URL(request.url);
    const pathname = decodeURIComponent(url.pathname === '/' ? '/index.html' : url.pathname);
    const filePath = path.normalize(path.join(studioRoot(), pathname.replace(/^\/+/, '')));
    const root = studioRoot();
    const relative = path.relative(root, filePath);
    if (relative.startsWith('..') || path.isAbsolute(relative)) {
      return new Response('Forbidden', { status: 403 });
    }
    try {
      const data = await fs.promises.readFile(filePath);
      return new Response(data, {
        headers: { 'content-type': contentType(filePath) },
      });
    } catch {
      const fallback = await fs.promises.readFile(path.join(root, 'index.html'));
      return new Response(fallback, {
        headers: { 'content-type': 'text/html; charset=utf-8' },
      });
    }
  });
}

function contentType(filePath) {
  switch (path.extname(filePath).toLowerCase()) {
    case '.html': return 'text/html; charset=utf-8';
    case '.js': return 'text/javascript; charset=utf-8';
    case '.css': return 'text/css; charset=utf-8';
    case '.json': return 'application/json; charset=utf-8';
    case '.wasm': return 'application/wasm';
    case '.svg': return 'image/svg+xml';
    case '.png': return 'image/png';
    case '.ico': return 'image/x-icon';
    case '.webmanifest': return 'application/manifest+json';
    default: return 'application/octet-stream';
  }
}

function createWindow() {
  mainWindow = new BrowserWindow({
    width: 1440,
    height: 920,
    minWidth: 1040,
    minHeight: 700,
    show: false,
    webPreferences: {
      preload: path.join(__dirname, 'preload.cjs'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: false,
    },
  });
  mainWindow.once('ready-to-show', () => mainWindow.show());
  void mainWindow.loadURL('rhwp://app/index.html').then(() => scheduleSmokeExit());
}

function scheduleSmokeExit() {
  if (smokeExitScheduled) return;
  const delay = Number(process.env.RHWP_DESKTOP_SMOKE_EXIT_MS || 0);
  if (!Number.isFinite(delay) || delay <= 0) return;
  smokeExitScheduled = true;
  setTimeout(() => app.quit(), delay);
}

function startMcpSidecar() {
  const bin = mcpBinaryPath();
  if (!fs.existsSync(bin)) return;
  const root = app.getPath('documents');
  mcp = spawn(bin, {
    stdio: ['pipe', 'pipe', 'pipe'],
    env: { ...process.env, RHWP_MCP_ROOT: root },
  });
  mcp.stdout.setEncoding('utf8');
  let buffer = '';
  mcp.stdout.on('data', (chunk) => {
    buffer += chunk;
    let newline = buffer.indexOf('\n');
    while (newline >= 0) {
      const line = buffer.slice(0, newline).trim();
      buffer = buffer.slice(newline + 1);
      if (line) handleMcpLine(line);
      newline = buffer.indexOf('\n');
    }
  });
  mcp.stderr.on('data', (chunk) => {
    console.warn(`[rhwp-mcp] ${String(chunk).trim()}`);
  });
  mcp.on('exit', () => {
    for (const { reject } of mcpPending.values()) reject(new Error('rhwp-mcp exited'));
    mcpPending.clear();
    mcp = null;
  });
}

function handleMcpLine(line) {
  let message;
  try {
    message = JSON.parse(line);
  } catch {
    return;
  }
  const pending = mcpPending.get(message.id);
  if (!pending) return;
  mcpPending.delete(message.id);
  if (message.error) pending.reject(new Error(message.error.message || 'MCP error'));
  else pending.resolve(message.result);
}

function callMcp(method, params) {
  if (!mcp?.stdin?.writable) throw new Error('rhwp-mcp is not running');
  const id = nextMcpId++;
  const payload = JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\n';
  return new Promise((resolve, reject) => {
    mcpPending.set(id, { resolve, reject });
    mcp.stdin.write(payload, 'utf8', (error) => {
      if (!error) return;
      mcpPending.delete(id);
      reject(error);
    });
  });
}

ipcMain.handle('rhwp:geminiGenerate', async (_event, body) => {
  const model = typeof body?.model === 'string' ? body.model : 'gemini-flash-latest';
  const input = typeof body?.input === 'string' ? body.input : '';
  const apiKey = typeof body?.apiKey === 'string' ? body.apiKey : '';
  const bearerToken = typeof body?.bearerToken === 'string' ? body.bearerToken : '';
  if (!input) throw new Error('input is required');
  if (!apiKey && !bearerToken) throw new Error('Gemini credential is required');
  const headers = { 'content-type': 'application/json' };
  if (apiKey) headers['x-goog-api-key'] = apiKey;
  else headers.authorization = `Bearer ${bearerToken}`;
  const normalizedModel = model.replace(/^models\//, '') || 'gemini-flash-latest';
  const response = await fetch(
    `https://generativelanguage.googleapis.com/v1beta/models/${encodeURIComponent(normalizedModel)}:generateContent`,
    {
      method: 'POST',
      headers,
      body: JSON.stringify({
        contents: [{ role: 'user', parts: [{ text: input }] }],
      }),
    },
  );
  const text = await response.text();
  let data = {};
  try {
    data = text ? JSON.parse(text) : {};
  } catch {
    data = { error: { message: text || response.statusText } };
  }
  if (!response.ok) {
    const detail = data?.error?.message || response.statusText;
    throw new Error(`HTTP ${response.status}: ${detail}`);
  }
  return data;
});

ipcMain.handle('rhwp:mcpRequest', async (_event, method, params) => callMcp(method, params));

app.whenReady().then(() => {
  registerStudioProtocol();
  startMcpSidecar();
  createWindow();
  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow();
  });
});

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') app.quit();
});

app.on('before-quit', () => {
  if (mcp) {
    mcp.kill();
    mcp = null;
  }
});
