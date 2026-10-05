# Instant CLI

Install on Linux x64 or ARM64 (Node.js 18 or newer):

```sh
npm install -g @instantos/cli
ins --help
```

Or run without a global install:

```sh
npx @instantos/cli --help
```

The `ins` command runs a precompiled Rust binary. npm selects a matching optional
binary package; keep optional dependencies enabled. The binaries use musl and
support both glibc distributions and Alpine Linux. macOS, Windows, ARMv7 and
Termux are not supported by this npm distribution.

This is a binary-only installation. External tools needed by individual commands
and systemd/desktop integration files must be installed separately.

[Documentation](https://instantos.io/docs/ins.html) ·
[Source and other installation options](https://github.com/instantOS/instantCLI)
