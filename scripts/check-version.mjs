import { readFile } from 'node:fs/promises';

const packageJson = JSON.parse(await readFile(new URL('../package.json', import.meta.url), 'utf8'));
const cargo = await readFile(new URL('../src-tauri/Cargo.toml', import.meta.url), 'utf8');
const tauri = JSON.parse(await readFile(new URL('../src-tauri/tauri.conf.json', import.meta.url), 'utf8'));
const cargoVersion = cargo.match(/^version\s*=\s*"([^"]+)"/m)?.[1];

if (!cargoVersion || packageJson.version !== cargoVersion || tauri.version !== packageJson.version) {
  console.error(`Version mismatch: package=${packageJson.version}, cargo=${cargoVersion}, tauri=${tauri.version}`);
  process.exit(1);
}

console.log(`Version ${packageJson.version} is consistent across package.json, Cargo.toml, and tauri.conf.json.`);
