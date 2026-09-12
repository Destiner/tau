import { readFileSync } from 'node:fs';

const version = JSON.parse(readFileSync('package.json', 'utf8')).version;
const tauriVersion = JSON.parse(
  readFileSync('src-tauri/tauri.conf.json', 'utf8'),
).version;
if (
  typeof version !== 'string' ||
  !/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(version)
) {
  throw new Error(
    'Release version must be a stable major.minor.patch version.',
  );
}
const cargo = readFileSync('src-tauri/Cargo.toml', 'utf8').match(
  /^version = "([^"]+)"$/m,
)?.[1];
if (tauriVersion !== version || cargo !== version) {
  throw new Error('Release versions disagree.');
}
console.log(version);
