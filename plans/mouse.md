I want to recreate the mouse slider assist from ./assist_old in `ins assist`. 
`ins assist` has a lot of its own utilities and ways of doing things, so adapt
it to those and read through other assist first for reference. The new version
should support X11 and wayland. 

In particular the brightness and audio slider `ins assist` are good references. 

For wayland, there is no standard way to do this
so this will have to be implemented on a per-compositor basis. There are alredy
utils for detecting compositor somewhere in the codebase, search and look for
those. The first wayland compositor I want to support is sway. 

Here is some info on the commands needed I researched. I want the mouse speed to
apply to all, so type:pointer should be used. 


```bash
swaymsg input <identifier> pointer_accel <value>
```
  

`<value>`**: A number between `-1.0` (slowest) and `1.0` (fastest).
Use `type:pointer` as the identifier to apply the speed to **all** mice/pointers.


There are existing utils for swaymsg handling, you might need to extend these. 

