// Reproducible application-owned Java 17 bridge; runtime needs no compiler.
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const { execFileSync } = require('node:child_process');
const root = path.resolve(__dirname, '..');
const directory = path.join(root, 'src-tauri/resources/signing');
const jar = process.env.ANDROID_DEVKIT_APKSIGNER_JAR || process.argv[2];
if (!jar || !fs.existsSync(jar)) throw Error('Pass the SDK Build-Tools lib/apksigner.jar path');
execFileSync(
  process.env.JAVAC || 'javac',
  [
    '--release',
    '17',
    '-encoding',
    'UTF-8',
    '-g:none',
    '-cp',
    jar,
    '-d',
    directory,
    path.join(directory, 'DevkitSigningBridge.java'),
  ],
  { stdio: 'inherit', windowsHide: true },
);
const hash = (name) =>
  crypto
    .createHash('sha256')
    .update(
      name.endsWith('.java')
        ? fs.readFileSync(path.join(directory, name), 'utf8').replaceAll('\r\n', '\n')
        : fs.readFileSync(path.join(directory, name)),
    )
    .digest('hex');
fs.writeFileSync(
  path.join(directory, 'bridge.json'),
  JSON.stringify(
    {
      javaRelease: 17,
      sourceSha256: hash('DevkitSigningBridge.java'),
      classSha256: hash('DevkitSigningBridge.class'),
    },
    null,
    2,
  ) + '\n',
);
console.log('Signing bridge compiled and hashed.');
