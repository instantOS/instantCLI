blueman-assistant is no longer a thing.
It has been replaced by blueman-manager.

The bluetooth service is also required for all of these, it should not be a
toggle, it should be a requirement for the entire setting page. Maybe a
requirement system with a very generic callback or trait should be introduced,
so that some settings pages prompt for requirements to be satisfied before they
open. Maybe a requirement array can be introduced to settings instead of a
package requirement array. Like

```
enum settingsSequirement {
    PackageRequirement(RequiredPackage),
    Customfunctioncomlpetest(fn() -> bool),
    //... other sensible stuff
}
```


The bluetooth applet is also not very
useful. This setting can be entirely removed. 

