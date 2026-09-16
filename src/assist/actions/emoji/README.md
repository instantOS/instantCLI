# Emoji catalog

`catalog.tsv` contains all 3,944 fully-qualified emoji sequences and their English
names from Unicode 17.0.0's
[emoji-test.txt](https://www.unicode.org/Public/17.0.0/emoji/emoji-test.txt).
Unqualified duplicates and standalone components are excluded; skin tones, flags,
keycaps, and ZWJ sequences are retained. Names are searchable descriptions, not a
CLDR keyword/synonym or localization database.

Regenerate from the repository root with `python3 scripts/update-emoji.py`.
Generation requires the network; building and using the picker do not. Updating
the Unicode version also requires reviewing the expected entry count in the
script and Rust tests.

The data is one embedded UTF-8 blob (~157 KiB including the Unicode license),
not a static table of pointers or a third-party emoji crate. It is parsed only
when the picker opens. No font/image data is embedded: install an emoji-capable
font for display. The license notice is included both here as `LICENSE` and in
the embedded catalog so binary-only distributions retain the notice.
