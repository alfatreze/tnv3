import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const read = (path) => readFileSync(fileURLToPath(new URL(path, import.meta.url)), 'utf8');
const pkg = JSON.parse(read('../package.json'));
const lock = JSON.parse(read('../package-lock.json'));
const tauri = JSON.parse(read('../src-tauri/tauri.conf.json'));
const cargo = read('../src-tauri/Cargo.toml').match(/^\[package\][\s\S]*?^version\s*=\s*"([^"]+)"/m)?.[1];
const cargoLock = read('../src-tauri/Cargo.lock').match(/^name = "tau-encoder"\nversion = "([^"]+)"/m)?.[1];
const versions = { package: pkg.version, packageLock: lock.version, packageLockRoot: lock.packages['']?.version, tauri: tauri.version, cargo, cargoLock };
const semver = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-(?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*)(?:\.(?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*))*)?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/;
if (!semver.test(pkg.version) || Object.values(versions).some((v) => v !== pkg.version)) {
  console.error('Invalid or inconsistent Tau Encoder versions:', versions);
  process.exit(1);
}
console.log(`Tau Encoder versions agree: ${pkg.version}`);
