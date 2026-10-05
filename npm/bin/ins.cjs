#!/usr/bin/env node
'use strict';

const { spawn } = require('node:child_process');

const packages = {
  'linux-x64': '@instantos/cli-linux-x64',
  'linux-arm64': '@instantos/cli-linux-arm64',
};
const platform = `${process.platform}-${process.arch}`;
const packageName = packages[platform];
if (!packageName) {
  console.error(`ins: unsupported platform ${platform}. npm builds support Linux x64 and ARM64.`);
  process.exit(1);
}
let binary;
try {
  binary = require.resolve(`${packageName}/bin/ins`);
} catch {
  console.error(`ins: missing ${packageName}. Reinstall @instantos/cli with optional dependencies enabled (npm install -g --include=optional @instantos/cli).`);
  process.exit(1);
}
const child = spawn(binary, process.argv.slice(2), { stdio: 'inherit' });
const signals = ['SIGINT', 'SIGTERM', 'SIGHUP'];
for (const signal of signals) {
  process.on(signal, () => child.kill(signal));
}
child.on('error', (error) => {
  console.error(`ins: could not start native binary: ${error.message}`);
  process.exit(1);
});
child.on('exit', (code, signal) => {
  if (signal) {
    process.removeAllListeners(signal);
    process.kill(process.pid, signal);
  } else {
    process.exit(code ?? 1);
  }
});
