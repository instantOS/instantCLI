Come up with a better architecture for Settings. 
Maybe Setting could be a trait, which has methods like apply. 
The settingdefinition struct would therefore not have to be manually created, or
even be removed all together, and the setting keys would not be strings which
are duplicated across the codebase. 

Apply and restore could share lots of code, if not be merged all together. 
Errors when applying settings could be shown to the user in a generic way. 

Right now adding new settings requires adding things in a bunch of different
places, this is bad DX. 

SOLID also gets violated, as restoring a setting has to be manually added for
each setting. Maybe the Setting trait could have a method which returns the
store key if the setting should be stored, and a generic restore loop could call
restore on all the settings. 

Read through all of the code and be creative. 


Keep in mind some settings just execute a program, they do not store any state and do not get restored. 