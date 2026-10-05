# instantOS package signing key

`instantos-signing-key.asc` is the public key exported from the vendored
`packages/instantos-keyring/instantos.gpg` in the instantOS/packages repository.
The master fingerprint is `E5C4740F883910B29694D6BE10DA645A82CF5206`; the initial
signing subkey fingerprint is `86D0589DC0EC55E4E5DFCE4436276C1C0A17CCD1`.

The Rust installer and `ins doctor fix instant-keyring` use this snapshot to
bootstrap pacman's configured GPGDir without network access. Chroot re-entry
makes the target's keyring the current one. Import merges the snapshot into
existing keys, preserving any newer subkeys and revocations.

The standalone shell installer embeds the same snapshot in
`scripts/install-src/instantos-keyring.sh`. After changing it, regenerate
`scripts/install.sh` with `scripts/build-install.sh` and update the website's
`public/install` and `public/install.sh` copies. The shell and Rust tests check
the snapshot and pinned fingerprints. Keep the instantOS repo's standalone
`repo.sh` bootstrap and ISO verifier in sync during signing subkey rotation.
