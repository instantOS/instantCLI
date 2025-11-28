Improvement for `ins arch`

The logs for the log uploader are fairly incomplete. 
The actual output of the commands being run is not in the logs.

Some improvement suggestions:

Store command runner stdout and stderr logs in log file. It is important that
the output is also shown in the terminal as it is now, if that is difficult,
thta takes priority over logging it, in case achieving both is difficult. 

The logs of chroot steps is just shown as a single entry. Chroot steps should
record their own logs, and the non-chroot `ins` instance should collect and
process (mostly concatenate) the logs of the chroot steps. 





