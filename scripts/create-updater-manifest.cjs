const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');

function create(directory, version, notes = '', publishedAt = new Date().toISOString()) {
  assert.match(version, /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/, 'Expected a stable version');
  const installer = `AndroidDevKit-${version}-setup.exe`;
  const root = path.resolve(directory);
  assert(fs.statSync(path.join(root, installer)).size > 0, 'Missing installer');
  const signature = fs.readFileSync(path.join(root, `${installer}.sig`), 'utf8').trim();
  assert(signature && signature.length < 16384, 'Missing or oversized updater signature');
  assert.equal(
    Buffer.from(signature, 'base64').toString('base64'),
    signature,
    'Invalid updater signature encoding',
  );
  assert(Number.isFinite(Date.parse(publishedAt)), 'Invalid update date');
  const manifest = {
    version,
    notes,
    pub_date: publishedAt,
    platforms: {
      'windows-x86_64': {
        signature,
        url: `https://github.com/whoarei/android-devkit/releases/download/v${version}/${installer}`,
      },
    },
  };
  fs.writeFileSync(path.join(root, 'latest.json'), JSON.stringify(manifest, null, 2) + '\n');
  return manifest;
}

if (require.main === module) {
  try {
    assert.equal(
      process.argv.length,
      4,
      'Usage: node create-updater-manifest.cjs <release-directory> <version>',
    );
    create(process.argv[2], process.argv[3], process.env.RELEASE_NOTES || '');
    console.log(`Signed updater manifest generated for ${process.argv[3]}.`);
  } catch (error) {
    console.error(`Updater manifest failed: ${error.message}`);
    process.exitCode = 1;
  }
}
module.exports = { create };
