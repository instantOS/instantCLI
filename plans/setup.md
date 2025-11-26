I want an `ins arch setup` command which sets up the instantOS configuration
(which is right now in postinstall) on top of an existing fresh Arch Linux
installation. 
It should be idempotent, so running it multiple times should not and should not
do any changes the second time around. 
This should reuse logic from postinstall, and postinstall should mostly just be
a chroot call to `ins arch setup`



