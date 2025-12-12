
The archlinux.org website is very unreliable. Implement a retry mechanism. 

If the mirrorlist cannot be received or is invalid, then default to
https://archlinux.org/mirrorlist/all/https (which contains all mirrors commented
out, which then need to be uncommented) And if that fails, use
/etc/pcaman.d/mirrorlist

The mirror region question should be skipped if it cannot be fetched from
archlinux.org. 

This might not be trivial, as the list is fetched by the provider.
For some questions, the provider failing to fetch its data is fatal. For example,
if the disk info provider does not return anything, we cannot select a disk to
install to and the entire installation process should be cancelled (after
communicating to the user why that is). On the other hand, if we cannot build a
region-specific mirrorlist, installation can proceed with the default
mirrorlist. Ensure that this behavior is modeled well in this project. 




