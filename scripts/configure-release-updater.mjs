import { readFile, writeFile } from 'node:fs/promises';

const path = new URL('../src-tauri/tauri.conf.json', import.meta.url);
const config = JSON.parse(await readFile(path, 'utf8'));
const hasPrivateKey = Boolean(process.env.TAURI_SIGNING_PRIVATE_KEY?.trim());

if (!hasPrivateKey) {
  config.bundle ??= {};
  config.bundle.createUpdaterArtifacts = false;
  console.log('TAURI_SIGNING_PRIVATE_KEY is not configured; building unsigned draft artifacts without updater signatures.');
} else {
  config.bundle ??= {};
  config.bundle.createUpdaterArtifacts = true;
  console.log('Tauri updater signing key detected; updater artifacts will be generated.');
}

await writeFile(path, `${JSON.stringify(config, null, 2)}\n`, 'utf8');
