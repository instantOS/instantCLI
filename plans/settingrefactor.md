Come up with a better architecture for Settings. 
Maybe Setting could be a trait, which has methods like apply. 
The settingdefinition struct would therefore not have to be manually created, or
even be removed all together, and the setting keys would not be strings which
are duplicated across the codebase. 

