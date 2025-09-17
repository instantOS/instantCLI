# Are there really none?

We are now years into the transition to wayland, yet one problem still remains: 
There is no dmenu replacement. Instantmenu is based on dmenu, which remains
stuck on X11, which is becoming a problem. 
You might say 'look at fuzzel/rofi/whatevershinynewthing', but they are not what
dmenu was truly good at: Not interrupting my workflow. Instantmenu was truly
tiny. And the very first thing it did was to grab my keyboard input. 
I could make it part of my typing flow, as I could use it purely from muscle
memory without thinking about it. This is not possible with the wayland
equivalents of it. They all take a variable and non-negligible amount of time to
load. This means that if I continue typing before they loaded, the first few
keystrokes will go to whatever application is currently in focus. Depending on
what that application is, this could be annoying to disastrous. It also means
those characters are not in the menu when it eventually opens. Double bummer. 

# Why did nobody develop that?

Take the following with a grain of salt, I don't entirely know what I'm talking
about, but to me it seems plausible. I have considered writing my own wayland
compositor, so I've read a few things about wayland, but I have barely scratched
the surface, and it was enough to understand I don't have the time to do that. 

As far as I can tell, wayland is more complex than Xorg for the end user. I know
that Xorg was spaghetti, but it was spaghetti which was developed at a time when
hardware was limited, applications were more basic under the hood, and memory
and storage usage mattered. The end result when it worked was simpler to use
than wayland. Sue me. Wayland seems to be an ever moving target, people don't
like to just use it, and instead rely on fairly heavy libraries to abstract it
away. 

All of these libraries are meant for applications which you open and and then
use for a while. They're not meant for utilities following the unix philosophy,
they're large and okay with 80% feature overlap with some other thing and 40%
feature overlap with almost all other things. 

Ironically, the fastest menu I have found on wayland is still dmenu, but because
it runs through xwayland, there's still a noticeable delay, and all sorts of
weird compositor specific issues, like the mouse having to be on the menu or the
menu being placed in the middle of the screen. 

# New approach

I am giving up opening a new window and expecing it to be fast enough to not
interrupt my typing. Instead, I will keep a window open and automatically show
or hide it based on whether menu input is currently required. 

An instance of the menu will run permanently in a scratchpad terminal. 
Questions can be sent to that menu, which will then make the terminal visible,
show the menu just like the normal CLI version would, and upon selection or
cancellation, hide the terminal and send back the chosen item. 


## Server

### Here's weird wacky pseudocode

```
- Server is running inside a terminal on the scratchpad, listens for menu requests
- on request:
    - show scratchpad terminal
    - open menu prompt
    - wait for menu selection
    - close menu prompt
    - hide scratchpad
    - send back selection message
```


### Currently open issues

For secure IPC between client and server, use Unix domain sockets. This ensures only processes with the same privilege level can access it, similar to Hyprland exec. Use the xdg crate to create the socket file in the appropriate runtime directory, such as $XDG_RUNTIME_DIR/instantmenu.sock. 

How can I deal with large inputs? If the menu receives tens of thousands of
lines of input via stdin, the client also needs to get that to the server. 
How can I do that in a way which is performant? Does that matter?

What happens when the server dies?
What happens when the scratchpad is closed or hidden by the user?
The server should be able to detect that and send a cancelled or serverclosed
message to the client. 

## Scratchpad

The server detects, which compositor or DE it is running on and adjusts the
commands which are needed to show or hide the scratchpad accordingly. This is
abstracted away so other parts of the server can just call some method to toggle
the scratchpad. 

### Sway

Sway does not have named workspaces, so the way this could be implemented is to
use some very high numbered workspace like 98. 

I also need to look into creating window rules so the menu window spawns on the 
right workspace.

### Hyprland

Hyprland has named workspaces called "special" workspaces, and can create window
rules using the hyprctl command. 

Set up a rule which spawns 'instantmenu' or class windows on the special workspace and also makes them centered. 
Use the togglesomethingsomething command to toggle the scratchpad.


## Client

Check server existence, if server is not running, start a new server in the
background. Then wait for existence (with a timeout), and then send the menu
request. 

A menu request should contain all the information needed to open the menu with
the same options as when running the menu directly in the terminal. This means
transmitting a list of items and options. The server should send back the item
chosen (or that the user has cancelled the menu). It should wrap the same enum
which is being used in the terminal version. The thing running in the server
terminal should be as equivalent to the normal version as possible. 

# Architecture

I would like this to stay in the CLI, and have a subcommand called menuserver
which opens up a scratchpad terminal which then runs the command `instant
menus server --inside` or has some other way of detecting if it should spawn a
scratchpad terminal or if it is running within the scratchpad terminal. 

The `instant menu` command should then have an option called `--gui` or
`--client` which then makes it send the menu request to the server instead of
displaying the menu itself (and spawns the server in the background if it is not
already running). 


