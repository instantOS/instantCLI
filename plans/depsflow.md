# Issue with the `ins game deps` command

When creating a new dependency using the `ins game deps add` command, the
depdendency is then shown as "not installed" when running `ins game deps list
<gamename>`

The dependency should be registered as installed after adding it, obviously the
machine which it originates from has the dependency. Fix this, make sure the
flow is logical, easy to follow and non-convoluted and accounts for edge cases.
Good DX is important, backwards compatibility is not.



