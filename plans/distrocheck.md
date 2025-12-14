Some setting should only he available on specific operatung sysyems or
distributions. There should be an easy way to optionally declare a subset
of distribitions where a srtting is available. upon opening a setting, if
the setting specifies it is only available on a specifig set of
distributions, detect the current distribution and check if it is in the set of
supported ones, or the distro it is based on is. If it is not, then show a
messahe describing that the setting is not available, along with a list
of supported distributions.
If a setting does not specify any dostrobution, it should be assumed
compatible with any distribution and not check the distro. Refactors and
breakint changes allowed
