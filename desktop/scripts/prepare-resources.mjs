import { copyFile, cp, mkdir, rm } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const desktopRoot = path.resolve(__dirname, '..');
const repoRoot = path.resolve(desktopRoot, '..');
const resourcesRoot = path.join(desktopRoot, 'build-resources');
const studioDist = path.join(repoRoot, 'rhwp-studio', 'dist');
const mcpName = process.platform === 'win32' ? 'rhwp-mcp.exe' : 'rhwp-mcp';
const mcpCandidates = [
  path.join(repoRoot, 'target', 'desktop-release', mcpName),
  path.join(repoRoot, 'target', 'release', mcpName),
  path.join(repoRoot, 'target', 'debug', mcpName),
];
const mcpBin = mcpCandidates.find((candidate) => existsSync(candidate));

if (!existsSync(studioDist)) {
  throw new Error(`Missing rhwp-studio build output: ${studioDist}`);
}
if (!mcpBin) {
  throw new Error('Missing rhwp-mcp binary. Run: cargo build --profile desktop-release --bin rhwp-mcp');
}

await rm(resourcesRoot, { recursive: true, force: true });
await mkdir(path.join(resourcesRoot, 'bin'), { recursive: true });
await cp(studioDist, path.join(resourcesRoot, 'studio'), { recursive: true });
await copyFile(mcpBin, path.join(resourcesRoot, 'bin', mcpName));
