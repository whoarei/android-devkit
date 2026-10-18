const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');
const crypto = require('crypto');
const root = path.resolve(__dirname, '..');
const metadata = JSON.parse(
  execFileSync(
    'cargo',
    [
      'metadata',
      '--manifest-path',
      'src-tauri/Cargo.toml',
      '--locked',
      '--offline',
      '--format-version',
      '1',
      '--filter-platform',
      'x86_64-pc-windows-msvc',
    ],
    { cwd: root, encoding: 'utf8', maxBuffer: 32 * 1024 * 1024 },
  ),
);
const npm = JSON.parse(
  process.platform === 'win32'
    ? execFileSync('cmd.exe', ['/d', '/s', '/c', 'pnpm licenses list --prod --json'], {
        cwd: root,
        encoding: 'utf8',
      })
    : execFileSync('pnpm', ['licenses', 'list', '--prod', '--json'], {
        cwd: root,
        encoding: 'utf8',
      }),
);
const nodes = new Map(metadata.resolve.nodes.map((n) => [n.id, n]));
const seen = new Set();
function visit(id) {
  if (seen.has(id)) return;
  seen.add(id);
  for (const dep of nodes.get(id)?.deps ?? [])
    if (dep.dep_kinds.some((k) => k.kind !== 'dev')) visit(dep.pkg);
}
visit(metadata.resolve.root);
const entries = [];
const upstream = JSON.parse(
  fs
    .readFileSync(path.join(root, 'src-tauri/resources/licenses/upstream.json'), 'utf8')
    .replace(/^\uFEFF/, ''),
);
function add(name, version, license, repository, directory, extra) {
  const files = fs
    .readdirSync(directory)
    .filter(
      (f) =>
        /^(licen[cs]e|copying|notice)([.\-_]|$)/i.test(f) &&
        fs.statSync(path.join(directory, f)).isFile(),
    );
  if (extra && fs.existsSync(path.join(directory, extra)) && !files.includes(extra))
    files.push(extra);
  const texts = files.map((f) => `[${f}]\n${fs.readFileSync(path.join(directory, f), 'utf8')}`);
  if (!texts.length) {
    const component = name.replace(/^Rust: /, '');
    const fallback = upstream.find(
      (s) =>
        s.name === component ||
        (s.name === 'webview2-com' && component.startsWith('webview2-com-')),
    );
    if (fallback) {
      const file = path.join(root, 'src-tauri/resources/licenses', fallback.file);
      const bytes = fs.readFileSync(file);
      if (crypto.createHash('sha256').update(bytes).digest('hex') !== fallback.sha256)
        throw Error('License checksum mismatch');
      texts.push(`[Upstream license: ${fallback.url}]\n${bytes.toString('utf8')}`);
    }
  }
  entries.push({
    name,
    version,
    license: license ?? 'UNDECLARED',
    repository,
    sourcePackage: name.startsWith('Rust: ')
      ? `https://crates.io/api/v1/crates/${name.slice(6)}/${version}/download`
      : repository,
    texts,
    fullTextLocated: texts.length > 0,
  });
}
for (const p of metadata.packages
  .filter((p) => seen.has(p.id) && p.source)
  .sort((a, b) => a.name.localeCompare(b.name)))
  add(
    `Rust: ${p.name}`,
    p.version,
    p.license,
    p.repository,
    path.dirname(p.manifest_path),
    p.license_file,
  );
for (const p of Object.values(npm)
  .flat()
  .sort((a, b) => a.name.localeCompare(b.name)))
  add(`npm: ${p.name}`, p.versions.join(', '), p.license, p.homepage, p.paths[0]);
const locks = ['pnpm-lock.yaml', 'src-tauri/Cargo.lock']
  .map(
    (f) =>
      `${f}: ${crypto
        .createHash('sha256')
        .update(fs.readFileSync(path.join(root, f)))
        .digest('hex')}`,
  )
  .join('\n');
const output =
  `Android DevKit third-party notices\nGenerated from the locked Windows normal/build Rust dependency graph and production npm packages. Includes build-time dependencies as a conservative superset. Android SDK and optional Java are downloaded separately under their displayed licenses. Unmodified covered Rust source is available from each exact-version source-package URL below.\n${locks}\n\n` +
  entries
    .map(
      (p) =>
        `${'='.repeat(72)}\n${p.name} ${p.version}\nLicense: ${p.license}\nSource: ${p.repository ?? ''}\nSource package: ${p.sourcePackage ?? ''}\n${p.texts.join('\n\n') || 'License expression declared by package; full text not found at package root. Review the upstream source before public release.'}`,
    )
    .join('\n\n');
fs.writeFileSync(path.join(root, 'src-tauri/resources/THIRD_PARTY_NOTICES.txt'), output);
fs.writeFileSync(
  path.join(root, 'src-tauri/resources/third-party-manifest.json'),
  JSON.stringify(
    {
      entries: entries.map(({ texts, ...p }) => p),
      missingRootLicenseTexts: entries.filter((p) => !p.fullTextLocated).map((p) => p.name),
    },
    null,
    2,
  ) + '\n',
);
console.log(
  `Collected ${entries.length} dependency notices; ${entries.filter((p) => !p.fullTextLocated).length} require upstream text review.`,
);
