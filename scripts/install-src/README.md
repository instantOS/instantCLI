# Installer sources

These files are the maintainable source for the public, curlable
`scripts/install.sh` installer.

`install.sh` is the entrypoint template. Lines in the form
`# @include file.sh` are expanded recursively by `../build-install.sh`. Includes
must be relative to this directory and cannot use `..` path components.

After changing a source file, rebuild and validate the distribution artifact:

```sh
./scripts/build-install.sh
./scripts/build-install.sh --check
./tests/test_install_script.sh
```

Commit the generated `scripts/install.sh` alongside source changes so the raw
GitHub URL and deployment recipes always serve the current installer.
