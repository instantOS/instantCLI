
I tested `ins arch install` and `ins arch exec` on a live arch iso and ran into
a few problems. 

The `ins arch install` command should be able to run on a live arch iso

It should check if all its dependencies are installed. 


If it gets detected that it's running on a live arch iso, it should install any missing dependencies automatically (it has root in that case)

Look what these dependencies are. There are already dependency utilities used in
`ins assist` and `ins settings` that can be reused for this. 
There is one special case: On a live iso, you cannot assume fzf is installed, and on a live iso
you do not need to ask, you can just install it. 


It also needs to be ensured the dependencies are installed on the installation itself, otherwise `ins arch exec` cannot call itself in the chroot, as ins calls will fail. 
`ins` fails to run if libgit2 is not installed, so include that on the pacstrap.
Any other dependencies should also be part of the pacstrap. Any calls inside the
chroot should not pop up anything interactive, keep that in mind when handling
dependencies, in particular menu_utils should be avoided. 


