'use strict';

// Publish binaries before the launcher. Completed versions are skipped on retries.
const { readFileSync } = require('node:fs');
const { resolve } = require('node:path');
const { spawnSync } = require('node:child_process');

const root = resolve(process.argv[2]);
for (const directory of ['cli-linux-x64', 'cli-linux-arm64', 'cli']) {
  const cwd = resolve(root, directory);
  const { name, version } = JSON.parse(readFileSync(resolve(cwd, 'package.json')));
  const lookup = spawnSync('npm', ['view', `${name}@${version}`, 'version', '--json'], { encoding: 'utf8' });
  if (lookup.error) throw lookup.error;
  if (lookup.status === 0 && JSON.parse(lookup.stdout) === version) {
    console.log(`Already published: ${name}@${version}`);
    continue;
  }
  if (lookup.status !== 0 && !/E404/.test(lookup.stderr + lookup.stdout)) {
    throw new Error(`Cannot query ${name}: ${lookup.stderr || lookup.stdout}`);
  }
  const tag = version.includes('-') ? 'next' : 'latest';
  const result = spawnSync('npm', ['publish', '--access', 'public', '--provenance', '--tag', tag], { cwd, stdio: 'inherit' });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}
