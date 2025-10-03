# Plans for settings

## Better serialization

Right now the serialization to settings toml is a bit awkward, using string values with dots in
them instead of a real hierarchy and struct depicting that. 
Come up with architectural adjustments which make this cleaner while still
retaining good DX and the ability to easily add new settings entries.

