# change remove command

make the `instant dot repo remove` command remove the repo files by default, add
a --keep-files flag to keep the files. Do not keep backwards compatibility.


# rework test utils

the test utils are a bit messy.
They should provide at least the following:

- set up a test environment consisting of a  temporary directory, and provide a wrapper which runs commands with
that directory as the $HOME directory. 
- function to run the `instant` cli. The absolute path to the compiled CLI from
  ./target should be found, and then the function should just run the CLI with
the absolute path. The entire test should run with the fake home directory and
be unaware that the home directory has been changed. This means running tests
without that wrapper is discouraged. Create another function which exits a test
if it detects that it is not within the wrapper. Because the home directory in
the test is fake, using custom paths for the database and config file or repo
path is not needed. The test utils and the tests can be radically simplified this way. 
- function to set up a dotfile repository to be cloned, containing some dotfiles
  for made up applications.

all tests should show the output of all commands, so a user or AI agent can look
over them to see if the program behaves correctly. Automated verification is not
needed for everything. Simple heuristics like checking if a file exists, two
files are different or a keyword is or is not in the output are sufficient.
Create utils for doing that. Make sure that the keyword checks do not prevent
the output from being printed.
