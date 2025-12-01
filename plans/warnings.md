Add the ability to add conditional warnings and messages before and between
questions in the `ins arch install` flow.

They should use the menu utils message feature.
These can just be dismissed by the user, they are not questions and do not
generate data. They can depend on question answers, in which case their
conditions should be calculated after the question(s) they depend on is answered.

They can also not depend on questions at all, in which case their conditions
should be evaluated before starting the installation. Each warning should be
shown only once as soon as its condition is 

The first warning is that if virtualbox is detected, the installer should state
that wayland does not work properly in virtualbox. 

The second warning is that if the encryption password is shorter than 4 characters, a
warning that it is insecure should be shown. 

