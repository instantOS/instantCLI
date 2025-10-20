I want the project to compile with less warning, but I do not want dirty fixes
or to just supress the warnings. 

Fix the following:

run `cargo clippy --fix --allow-dirty` and fix
- all 'identical if else blocks' warnings
- calls to `push` immediately after creation (if appropriate)

Look at what is good for the maintainability of the code and the DX, think about
the changes you are making, do not get rid of the wardnings at all cost. 

also run `cargo check`

The unused fields in the restic wrapper (BackupSummary and so on) are okay, they
might be used in the future, you can suppress those warnings for now. Just suppress those. 


check which way is used to get text input using the menu utils (fzf wrapper) and
if there are duplicated or unused methods which can be removed. 

Other 'unused' warning can be ignored, do not suppress them, just leave them

