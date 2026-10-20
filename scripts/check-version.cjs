const fs = require('node:fs');
const assert = require('node:assert/strict');
const path = require('node:path');
const root = path.resolve(__dirname, '..');
const read = (file) => fs.readFileSync(path.join(root, file), 'utf8');
const { version } = JSON.parse(read('package.json'));
assert.match(version, /^\d+\.\d+\.\d+$/, 'Use a stable semantic version');
assert.equal(JSON.parse(read('src-tauri/tauri.conf.json')).version, version, 'Tauri version');
assert.equal(
  read('src-tauri/Cargo.toml').match(/^version = "([^"]+)"/m)?.[1],
  version,
  'Cargo version',
);
assert.equal(
  read('src-tauri/Cargo.lock').match(/name = "android-devkit"\r?\nversion = "([^"]+)"/)?.[1],
  version,
  'Cargo lock version',
);
const tag =
  process.argv[2] || (process.env.GITHUB_REF_TYPE === 'tag' ? process.env.GITHUB_REF_NAME : '');
if (tag) assert.equal(tag, `v${version}`, 'Release tag must match the application version');
console.log(`Application version ${version}${tag ? ` matches ${tag}` : ' is consistent'}.`);
