# Issues

Odd behaviors I found during testing which should be examined. 

## Outdated

after I run `instant dot add file.txt`
and then `instant dot status`, the file immediately shows as outdated
The file has the same content in the source and the target, because the source
was just created from the target. Some logic here is odd. 

