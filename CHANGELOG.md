# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.13.13](https://github.com/instantOS/instantCLI/compare/v0.13.12...v0.13.13) - 2026-02-22

### Fixed

- fix release?

## [0.13.12](https://github.com/instantOS/instantCLI/compare/v0.13.11...v0.13.12) - 2026-02-20

### Other

- better sync status

## [0.13.11](https://github.com/instantOS/instantCLI/compare/v0.13.10...v0.13.11) - 2026-02-19

### Fixed

- fix ownership issue
- fix arm build
- fix steam deck using wrong path
- fix more css
- fix video code block parsing
- fix timestamp parsing issue
- fix ffmpeg progress

### Other

- ins video preview
- init slide format
- clean up js
- better wrapping
- better looking title code blocks
- better code block scale handling
- prettier status bar
- refactor overlay filters
- use filterchain more
- init filterchain refactor
- init better ffmpeg progress bar
- remove redundant index function
- remove unused profile
- use new sourcemap
- refactor filter thingy
- cleaner categorization
- remove redundant struct
- add renderjob abstraction
- init render structure refactor
- refactor render handle
- remove redundant map
- document better
- remove old function

## [0.13.10](https://github.com/instantOS/instantCLI/compare/v0.13.9...v0.13.10) - 2026-02-17

### Fixed

- fix sync again, fix settings being slow

### Other

- add deb packaging
- dont do silly shiat
- add db index
- init lazygit command
- dedupe username validation and current_exe
- remove redundant deps
- refactor slider
- refactor all thingy
- add `ins menu all`

## [0.13.9](https://github.com/instantOS/instantCLI/compare/v0.13.8...v0.13.9) - 2026-02-17

### Fixed

- fix screen recording failure on gnome
- fix mouse sensitivity detection
- fix whisper python compat issues

### Other

- make use of message server feature
- init menu server message
- init settings assist
- add gnome keyboard settings support
- add ins doctor ssh check
- better behavior for update on non-read-only repos
- better ffmpeg handling
- cleanup
- some renames
- consolidate video source thingy
- consolidate frontmatter handling
- remove reduntant metadata
- structure video dims
- init timewindow refactor
- make clippy happier

## [0.13.8](https://github.com/instantOS/instantCLI/compare/v0.13.7...v0.13.8) - 2026-02-11

### Fixed

- fix back menu
- fix appimage on SteamOS game mode
- fix keywords on wide terminals
- fix termux filter

### Other

- simplify add.rs
- rename
- refactor discovery to trait
- fmt
- better azahar previews
- init azahar discovery
- add duckstation previews
- duckstation discovery
- cleanup
- standardize some stuff
- split manager.rs
- refactor manager.rs
- clean up function names
- pcsx2 discovery
- better eden behavior
- filter already added games
- init eden discovery
- better shortcut thingy
- add emudeck pcsx2 support
- better builder
- clean up duplicates
- add to desktop feature
- clippy
- remove old test
- better steam shortcut handling
- better self-update
- better flatpak manage flow
- some cleanup
- prettier doctor
- prettier flatpak menu
- better flatpak list

## [0.13.7](https://github.com/instantOS/instantCLI/compare/v0.13.6...v0.13.7) - 2026-02-10

### Fixed

- fix flatpak
- fix snap menu
- fix preview matching
- fix bugs
- fix separators

### Other

- more detailled previews
- more clear previews
- prettier snap installer
- better categorization
- prettier installable packages menu
- reinvent less menu stuff
- less unnecessary string
- consolidate fzf exit checking
- better slider builder
- remove unused code
- cleaner args
- init support for separators
- consolidate stats feature
- better video project menu
- better post render menu
- remove unneeded function
- cleaner audio resolver
- rename oddly named functions
- refactor disk execution
- refactor compiler.rs
- rename to better names
- rename dotfilerepo
- Merge branch 'dev' of github.com:instantOS/instantCLI into dev
- better builder offer behavior
- better clone behavior
- better dev previews
- init dev menu
- refactor discover_dotfiles
- refactor dotfile merge
- more sensible merge
- add keyword
- refactor snapshots
- even better snapshot preview
- better snapshot preview

## [0.13.6](https://github.com/instantOS/instantCLI/compare/v0.13.5...v0.13.6) - 2026-02-08

### Fixed

- fix mouse sensitivity
- fix video project thingy
- fix
- fix
- fix missing audio
- fix math in reels mode
- fix
- fix b-roll not playing frames
- fix sway picker
- fix toggle
- fix silence parsing
- fix
- fix plymouth application
- fix fzf wrapper
- fix yes/no menu
- fix modified detection

### Other

- init ability to remove from steam
- better previews
- ask for confirmation
- offer command removal
- better game menu setup flow
- case insensitive appimage search
- init `add to steam` support
- azahar support for ins game
- more clippy
- make use of transform abstraction
- cleaner OS detection
- make clippy happy
- less duplication
- add duckstation/mgba support to ins game launch
- init PCSX2
- update location
- init launch builder
- refactor package install
- remove unused field
- remove unused shit
- faster hashing algorithm
- better locale preview
- type safe sliders
- add ability to create a group
- better previews
- better sudo remove warning
- add sudoers setting
- more keywords, pacman cache clear option
- more refactor
- init refactor
- better normalize
- use podcast preset
- make normalization more aggressive
- improve title scaling
- init removal of separate title handling
- make b-roll distinguishable
- more b-roll tests
- change how broll works
- fmt
- Merge branch 'dev' of github.com:instantOS/instantCLI into dev
- merge refactor
- better menu
- add subtitle support for standard mode
- more display traits
- better sink flow
- implement display for some enums
- better preprocessor preview
- fmt
- better vidoe name conflict resolution
- better flow
- add recording option
- better menu flow
- better video preview
- video menu refactor
- restructure video menu
- change from : to @ for sources
- remove legacy frontmatter support
- init ffmpeg compile work
- more multi source support
- cli commands for multi source
- init support for multiple video sources
- file picker previews
- add path suggestions to picker
- init video menu
- init units menu
- remove dead code
- allow overlapping units
- refactor flatpak installer
- add unit support to `ins status`
- refactor grub handling
- refactor mkinitcpio requirements
- add flatpak dep
- refactor ask command
- init detection of existing answers
- move out previews
- more consistent error handling
- make ins arch use correct wrapper
- init review menu previews
- better settings location
- remove old mod
- better positioning
- init screen freeze thingy
- refactor recording source settings
- refactor ins assist mode generation
- refactor sway setup
- refactor dotfile source detection
- refactor keychord TUI
- refactor yazi run
- more keywords
- better snap search
- Merge branch 'dev' of github.com:instantOS/instantCLI into dev
- init snap support
- refactor swapescape and args
- refactor mirror
- add ability to mirror
- init better x11 mirroring
- init X11 support for display mirror
- add x11 support for screen recordings
- init ffmpeg dep
- better lang preview

## [0.13.5](https://github.com/instantOS/instantCLI/compare/v0.13.4...v0.13.5) - 2026-01-29

### Fixed

- fix audio source building
- fix sway bug
- fix mixed previews

### Other

- better naming
- better audio source settings
- add recording framerate setting
- recording refactor
- refactor audio source handling
- add default mode
- add audio source previews
- add preview support to checklists
- better source selection
- init audio setting
- file action
- use existing utils
- init screenrecording
- better app selection preview
- init appimage management setting
- init bluetooth detection setting
- better structure
- better keyboard menu
- better previews
- add ts assist, make apply restore wallpaper
- prettier `ins arch` menu
- better xdg mime detection
- refactor preview
- actual command
- init migration away from bash previews
- better keyboard previews
- better keyboard settings

## [0.13.4](https://github.com/instantOS/instantCLI/compare/v0.13.3...v0.13.4) - 2026-01-27

### Other

- add preview wrapping for doctor

## [0.13.3](https://github.com/instantOS/instantCLI/compare/v0.13.2...v0.13.3) - 2026-01-26

### Fixed

- fix picker
- fix subdir enablement bug
- fix crash
- fix help tree order
- fix unsupported

### Other

- prettier repo select
- better args
- better structure
- better status
- better status message
- fmt
- more intuitive UX
- less io
- display status in alternative picker
- better alternative flow
- init better auto option and checkbox actions
- fmt
- refactor subdir actions
- better empty default handling
- better default subdir handling
- default subdirs action
- add unsupported thingy
- better help menu
- better assist help tree
- add sync all option
- better looking `ins game menu`
- better label
- better ins game previews
- better edit pattern
- better text edit helper
- refactor long functions
- refactor repo actions
- better preview
- better repo actions
- prettier preview
- check systemd

## [0.13.2](https://github.com/instantOS/instantCLI/compare/v0.13.1...v0.13.2) - 2026-01-22

### Fixed

- fix file extension detection
- fix the shell preview builder escaping
- fix
- fix repo readable action
- fix circular dependency issue

### Other

- add more common file extensions
- better archive manager setting
- better group descriptions
- improve styling
- prettier subcategory preview
- prettier color preview
- better back button preview
- add better colored wallpaper preview
- better source for gtk themes
- init faillock doctor
- skip topgrade self-update
- better looking command settings
- better bluetooth settings preview
- better repo preview
- improve self_update
- implement auto dualboot
- add doctor concurrency flag
- optimize config/db handling
- prettier confirm
- better looking confirm menu
- better disk previews
- add more previews
- previews for `ins arch` questions
- refactor `ins arch`
- more video refactor
- init video refactor
- improve mime type preview
- dot repo refactor
- refactor defaultapps

## [0.13.1](https://github.com/instantOS/instantCLI/compare/v0.13.0...v0.13.1) - 2026-01-19

### Other

- improve audio player setting
- fmt

## [0.12.2](https://github.com/instantOS/instantCLI/compare/v0.12.1...v0.12.2) - 2026-01-15

### Fixed

- fix menu loop
- fix dotfile dir being hidden
- fix which

### Other

- fmt
- add dotfiles to settings
- better alternative creation
- better orphan handling
- better error handling
- refactor alternative menu
- better override preview
- pick up on odd overrides
- better preselection
- better menu loop
- get rid of insane type
- make clippy happy
- more sane destination menu
- add new file tracker for alternative menu
- add better alternative menu
- better subdir menu editing
- refactor add menu
- refactor ins dot alternative
- ins dot alternative create now supports dirs
- remove redundant code
- more refactors
- remove redundant helpers
- remove redundant helpers
- better package manager abstraction
- flatpak manage setting
- refactor doctor command
- better terminology
- add back option to menu
- add alternate files option to ins dot menu
- add support for listing alternatives in directory

## [0.12.1](https://github.com/instantOS/instantCLI/compare/v0.12.0...v0.12.1) - 2026-01-14

### Other

- clippy
- init fzf doctor check

## [0.11.2](https://github.com/instantOS/instantCLI/compare/v0.11.1...v0.11.2) - 2026-01-10

### Fixed

- fix empty vs invalid selection
- fix mistype crash
- fix cursor position being odd
- fix checklist
- fix responsive fzf
- fix piper
- fix json
- fix restore bug
- fix bugs
- fix header color
- fix padding
- fix overlay positioning
- fix issue

### Other

- make checklist read stdin
- init checklist
- fmt
- more ansi support
- migrate some previews
- init preview builder
- better fix viewing
- prettier fix menu
- improve doctor integration
- make settings responsive
- init responsive layout for previews
- add line wrapping to messages
- clippy
- add git config check
- limit buffer amount
- more friendly doctor settings
- better settings doctor integration
- add bazzite support
- add SteamOS support to `ins doctor`
- change fix list to args
- init better batch fixing
- better outpout for dot reset
- init `ins dot pull`
- less awful function names
- init completions check
- add descriptions to assist completions
- more custom completions
- add doctor name completions
- remove unnecessary loop
- init backup skipper
- add checkpoint checkout menu
- add close menu option
- fmt
- better game edit dialogues
- remove wrong deprecated markers
- better display settings UX
- init fzf refactor
- better header interface
- default padding
- better header styling
- better game menu styling
- add move entry
- better init prompt
- init move game
- add setup option to menu

## [0.11.1](https://github.com/instantOS/instantCLI/compare/v0.11.0...v0.11.1) - 2026-01-03

### Other

- better edit preview

## [0.10.10](https://github.com/instantOS/instantCLI/compare/v0.10.9...v0.10.10) - 2025-12-27

### Fixed

- fix clip overlap
- fix multiple transcodes
- fix code highlighting CSS
- fix image CSS
- fix image?
- fix block quotes
- fix pause transitions
- fix stuff?
- fix assumption of chronological order
- fix concatenation
- fix silence handling
- fix muted audio
- fix auphonic detection
- fix hashing
- fix whisperx
- fix swap escape not being used
- fix KDE color picker
- fix annoying message
- fix KDE erratic behavior
- fix kde scratchpad detection
- fix kwin scratchpad
- fix kwin scratchpad

### Other

- add bat check
- more noise reduction
- rename slides even more
- rename title cards to slides
- split planner
- init splitting huge planner file
- refactor video_planner
- use json instead of srt
- start migrating to json
- make render use preprocessed audio
- improve compression
- new local preprocessor
- add new plans
- better titlecard cli
- init better block quotes
- improve cache invalidation
- include JS version change
- refactor timeline planning
- better slide building
- better video pause behavior
- that escalated quickly
- add padding
- improve whisper?
- time based matching
- disable auphonic free on setup
- skip auphonic when free
- instantmenu rework
- split out instantmenu version
- support instantmenu ins assist
- split render.rs
- unify frontmatter handling
- refactor check.rs
- init ffmpeg module
- refactor catppuccin
- make check verify more
- refactor render
- move some files
- rename mods
- refactor timeline
- add check command
- more logging
- strip html comments
- recactor document
- refactor convert.rs
- refactor auphonic
- better code blocks
- better CSS
- hacky fix
- better title card handling
- avoid uploading entire video to auphonic
- Merge branch 'dev' of github.com:instantOS/instantCLI into dev
- Add videosetup plan
- Merge branch 'dev' of github.com:instantOS/instantCLI into dev
- ignore case in settings search
- init setting up udev rules
- clippy
- even better post dep install menu
- better post install terminal thing
- Fix KWin scratchpad status and visibility checks
- better messaging for vc assist
- fmt
- clippy
- make kwin scratchpad faster
- better messaging
- swapescape fix
- add swapescape to kde
- more attempted fixes
- enhance kwin script
- add swapescape support to gnome
- Refactor OperatingSystem to remove one-line wrapper functions
- Merge branch 'main' into dev

## [0.10.9](https://github.com/instantOS/instantCLI/compare/v0.10.8...v0.10.9) - 2025-12-15

### Added

- Implement scratchpad support for Gnome

### Other

- Add newline after save path in setup installation

## [0.10.8](https://github.com/instantOS/instantCLI/compare/v0.10.7...v0.10.8) - 2025-12-12

### Fixed

- fix script?
- fix signature issues
- fix settings sorting
- fix dark/light switching for GTK 3
- fix GTK4 not being reset correctly

### Other

- add fatal installer error message
- fmt
- implement mirrorlist fallback
- fallback plans
- settings refactor
- simpler deduplication approach
- deduplicate restore
- add gtk menu icons setting
- better batch terminology
- refactor fzf select
- fmt
- refactor dark/light mode setting
- refactor apply function
- more gtk icon light/dark theme variants
- init icon theme dark/light variants
- init new doctor check
- add 'install more' for browsers
- add 'install more' setting to default file managers
- add adapta theme
- replace outdated themes
- migrate to builder pattern for terminal launch
- unify qrcode terminal use
- further unify terminal use
- refactor terminal handling
- add network config to welcome
- better welcome app disabling experience
- add archive manager install more options
- fmt
- use nerd fonts
- init `install more`

## [0.10.7](https://github.com/instantOS/instantCLI/compare/v0.10.6...v0.10.7) - 2025-12-10

### Fixed

- fix handling
- fix non-UTF8 appearing

### Other

- add batch support to flatpak installations
- make instantASSIST handle package installation decline
- remove redundant messages
- handle package installation decline
- clippy
- clean up setting state computation
- batch package ensure requests
- clean up package installation flow

## [0.10.6](https://github.com/instantOS/instantCLI/compare/v0.10.5...v0.10.6) - 2025-12-10

### Fixed

- fix pacman-contrib missing
- fix instant repo order
- fix iso pacman
- fix package installation order
- fix dual boot efi dir
- fix ESP detection
- fix disk cache
- fix wrong partition being used?
- fix dual boot handling
- fix dual boot partition creation

### Other

- plans
- installmore idea
- add jank plans
- better checks
- set up default user with wallpaper
- better error management
- better error handling
- install packages earlier
- partition deletion detection
- cap swap so it's not absurd
- fmt
- use largest free region instead of total space
- I hate disk stuff
- another fix?
- better json
- reduce amount of needed cowspace
- more sfdisk fixes
- switch to json
- create dualboot root in appropriate space
- account for fragmented storage
- deduplicate some stuff
- check sizes after creating partitions
- proper types for partitions
- dynamically update shrinking message
- refactor disk preparation

## [0.10.5](https://github.com/instantOS/instantCLI/compare/v0.10.4...v0.10.5) - 2025-12-09

### Fixed

- fix GPU driver installation
- fix cfdisk for resizing
- fix indent
- fix efi partition detection
- fix settings previews

### Other

- clippy
- auto disk unmounting
- notify the user no resize is necessary
- handle free space appropriately
- use colored
- better messaging
- fmt
- refactor dual boot verifier
- check if partition can actually be used now
- init btrfs size detection
- more nerd fonts icons
- clippy
- account for unused space
- better detection of odd stuff
- improve dualboot detection
- more typesafe dualboot detection
- deduplicate checks
- threshold 10GB
- init resize_instructions
- remove custom dualboot slider
- init dualboot questions
- init UI
- remove redundant field
- add efi detection
- better output
- fmt
- init dual boot detection
- make git shorthands accept git commands
- better package messaging
- init sway display check
- fmt
- add qt reset setting
- add preview command to dark mode
- add moon icon
- init dark mode setting
- add instantwm interaction module

## [0.10.4](https://github.com/instantOS/instantCLI/compare/v0.10.3...v0.10.4) - 2025-12-07

### Fixed

- fix tests?

### Other

- fmt
- add a bit of timestamp tolerance
- Merge branch 'dev'
- init beads
- *(beads)* commit untracked JSONL files

## [0.10.3](https://github.com/instantOS/instantCLI/compare/v0.10.2...v0.10.3) - 2025-12-07

### Fixed

- fix race condition?
- fix pacman check with temp files
- fix GTK 4 symlink handling

### Other

- make pacman checks skip non-arch
- fmt
- add stale pacman dirs check
- better base distro detection
- better OS detection
- show skip hints
- add system doctor setting

## [0.10.2](https://github.com/instantOS/instantCLI/compare/v0.10.1...v0.10.2) - 2025-12-07

### Fixed

- fix spinner
- fix fixing logic
- fix colors

### Other

- add progress bar to `ins doctor`
- consolidate checking
- clean up privilege checks
- introduce checks for root-only tests
- add a fix function for SMART
- add pacman sync warning
- init drive health check
- add update check warning
- add swap check
- add fix hints for warnings
- add doctor check sorting
- better color handling
- add more statuses to doctor
- conditional check for instantOS repo
- add more doctor checks
- improve `ins arch setup`
- make sudo setup idempotent
- disable notifications on settings apply
- make user previews prettier
- sync PKGBUILD version
- release v0.9.3
- remove dead code

## [0.10.1](https://github.com/instantOS/instantCLI/compare/v0.10.0...v0.10.1) - 2025-12-06

### Other

- remove inventory crate
- clippy
- make setting tree the single source of truth for setting location
- init flatpak setting

## [0.9.2](https://github.com/instantOS/instantCLI/compare/v0.9.1...v0.9.2) - 2025-12-06

### Fixed

- fix install category not showing
- fix setting entry colors
- fix instantassist error

### Other

- remove autotheming setting
- remove unused constants
- make default layout settings actually work
- better multiple choice behavior
- update agents md
- add close button to settings
- make further use of terminal utils
- deduplicate terminal logic and boolean toggles
- init welcome app
- init welcome plan
- auto generate breadcrumbs
- remove unneeded default
- test builder pattern
- use builder pattern
- add icon color override
- add back removed settings
- clean up more old files
- fmt
- clean up settings which launch external commands
- clippy
- fmt
- remove old registry
- more cleanup
- remove old stuff
- deduplicate settings vectors
- more migration
- migrate settings UI
- migrate more settings
- migrate more settings
- migrate more settings
- init setting refactor
- add sway support for swapescape setting
- more plans
- add swapescape setting
- merge desktop and layout settings

## [0.9.1](https://github.com/instantOS/instantCLI/compare/v0.9.0...v0.9.1) - 2025-12-05

### Other

- fmt
- add path displays
- refactor writable repo handling
- remove old plan
- add git convenience helpers
- display modified repos path
- adjust pkgbuild

## [0.8.6](https://github.com/instantOS/instantCLI/compare/v0.8.5...v0.8.6) - 2025-12-03

### Fixed

- *(arch)* respect kernel selection and install correct headers/drivers
- fix info assists
- fix tests
- fix a lot of warnings

### Other

- fmt
- refactor custom kernel handling
- init X11 support for keyboard layouts
- init keyboard settings
- new plans
- init info assists
- clear up confusion
- clean up changelog
- fmt
- init some cleanup
- fmt
- better alias

## [0.8.5](https://github.com/instantOS/instantCLI/compare/v0.8.4...v0.8.5) - 2025-12-03

### Other

- add dot clone alias

## [0.8.4](https://github.com/instantOS/instantCLI/compare/v0.8.3...v0.8.4) - 2025-12-03

### Fixed

- fix CI, make default dotfile repo read-only

## [0.8.3](https://github.com/instantOS/instantCLI/compare/v0.8.2...v0.8.3) - 2025-12-03

### Fixed

- fix aliasing
- fix outline
- fix logo compositing
- fix random wallpaper
- fix swaymsg path quoting
- fix arg parsing

### Other

- init random wallpaper
- add wallpaper setting
- add dwm support for `ins wallpaper`
- X11 support for `ins wallpaper`
- make autostart apply wallpaper
- init sway wallpaper support
- init wallpaper command
- cleanup
- make ins dot use menu utils
- better icons
- better logging for read-only
- init read-only dotfile repos
- document stuff

## [0.8.2](https://github.com/instantOS/instantCLI/compare/v0.8.1...v0.8.2) - 2025-12-03

### Fixed

- fix ins binary conflict
- fix plymouth
- fix cfdisk sigint handling
- fix cfdisk again
- fix cfdisk

### Other

- make ins arch setup better
- ilovecandy
- add clipmenud dep
- tty is finicky
- better get_default
- add mirror shuffling

## [0.8.1](https://github.com/instantOS/instantCLI/compare/v0.8.0...v0.8.1) - 2025-12-02

### Fixed

- fix swaymsg bug
- fix mouse assist

### Other

- better icons
- refactor questions
- validate ESP partition
- make mouse accell flat
- init manual partitioning
- init mouse instantassist
- init color picker assist
- skip plymouth in minimal mode
- refactor minimal mode
- init minimal mode
- allow dynamic defaults for booleanquestions
- make lightdm question a boolquestion
- configure lightdm
- Merge branch 'dev' of github.com:instantOS/instantCLI into dev
- set GRUB theme

## [0.7.14](https://github.com/instantOS/instantCLI/compare/v0.7.13...v0.7.14) - 2025-12-01

### Other

- better password loop
- init warnings
- better inf arch info print
- add distro to `ins arch info`
- better looking `ins arch info`
- add architecture detection to instantARCH
- move more stuff to GpuKind
- add GpuKind enum
- init arch info
- add intel GPU support
- amd detection
- better vm support
- auto install drivers
- make plymouth on the default
- refactor boolean questions
- simplify initramfs handling
- init plymouth

## [0.7.13](https://github.com/instantOS/instantCLI/compare/v0.7.12...v0.7.13) - 2025-11-30

### Fixed

- fix LUKS for ins arch

### Other

- refactor repo init
- init support for stow/yadm repos

## [0.7.12](https://github.com/instantOS/instantCLI/compare/v0.7.11...v0.7.12) - 2025-11-28

### Fixed

- fix lint

## [0.7.11](https://github.com/instantOS/instantCLI/compare/v0.7.10...v0.7.11) - 2025-11-28

### Fixed

- fix scratchpad init on sway
- fix encryption not booting
- fix distro detection
- fix LUKS?
- fix logging

### Other

- increase cowspace
- init dev setup command
- init plan for devsetup
- add option to force log upload
- auto upload logs
- encryption still broken, attempt fix
- continue scrachpad refactor
- init refactor for scratchpad
- init scratchpad support for i3
- limit retry amount for pacman
- init pacman retry loop
- upload install script
- add some tests to grub config parsing
- experimental LUKS
- add password encryption question
- remove plaintext password from installation
- refactor logging
- init better logging
- Merge branch 'main' into dev

## [0.7.10](https://github.com/instantOS/instantCLI/compare/v0.7.9...v0.7.10) - 2025-11-27

### Other

- better error handling
- remove problematic test

## [0.7.9](https://github.com/instantOS/instantCLI/compare/v0.7.8...v0.7.9) - 2025-11-26

### Fixed

- fix icons

### Other

- add install script
- init "finished" menu
- improve live iso dep installation
- add missing deps

## [0.7.8](https://github.com/instantOS/instantCLI/compare/v0.7.7...v0.7.8) - 2025-11-26

### Other

- clippy

## [0.7.7](https://github.com/instantOS/instantCLI/compare/v0.7.6...v0.7.7) - 2025-11-26

### Fixed

- fix setting up the pacman repo
- fix default groups
- fix chroot bug

### Other

- change state location
- better chroot state handling
- detect and deal with TTY

### Security

- security plans

## [0.7.6](https://github.com/instantOS/instantCLI/compare/v0.7.5...v0.7.6) - 2025-11-26

### Fixed

- fix requirements on live iso

### Other

- Merge branch 'main' into dev
- add live testing stuff

## [0.7.5](https://github.com/instantOS/instantCLI/compare/v0.7.4...v0.7.5) - 2025-11-26

### Other

- update os-release
- add sway
- make autostart run nvidia-settings -l
- smarter timedatectl behavior
- make dotfile cloning idempotent
- smarter user detection
- be mindful of other display managers
- init `ins arch setup`
- use localectl instead of editing /etc...
- deduplicate internet check
- init autostart command

## [0.7.4](https://github.com/instantOS/instantCLI/compare/v0.7.3...v0.7.4) - 2025-11-25

### Fixed

- fix test for containers

### Other

- Merge branch 'dev'
- make archinstall command refuse non-arch distros
- init distro detection

## [0.7.3](https://github.com/instantOS/instantCLI/compare/v0.7.2...v0.7.3) - 2025-11-25

### Fixed

- fix test for CI
- fix typos
- fix deps

### Other

- rename repo.rs
- init postinstall step
- sync PKGBUILD version
- release v0.7.2
- add step dependency system
- init bootloader step
- better chroot handling
- init config step
- init fstab step
- init base step
- init mirrorlist fetching
- improve dry run output
- better disk verification
- make RAM determine swap space
- better command runner
- init disk step
- add dry-run mode
- scaffold install execution

## [0.7.2](https://github.com/instantOS/instantCLI/compare/v0.7.1...v0.7.2) - 2025-11-25

### Other

- better chroot handling
- init config step
- init fstab step
- init base step
- init mirrorlist fetching
- improve dry run output
- better disk verification
- make RAM determine swap space
- better command runner
- init disk step
- add dry-run mode
- scaffold install execution

## [0.7.1](https://github.com/instantOS/instantCLI/compare/v0.7.0...v0.7.1) - 2025-11-25

### Fixed

- fix output

### Other

- support AUR for package installation
- more display trait'
- refactor display for bootmode
- better output
- add more disk validation
- update deps
- better icons
- better icon
- mask password
- sort annotated values
- even better annotation provider
- better annotation provider
- init annotations
- add proper keymap provider
- more icons
- add icons
- refactor very long engine function
- better question fetching system
- trigger review at the end of question asking
- add question review
- add output option
- utf
- better locale ask
- implement real time zone provider
- better requirement handling
- add proper locale asking
- make arch install require root
- init disk provider
- init data provider system
- real mirrorlist parsing
- implement going back
- add list and ask commands
- add input validation
- init arch installer
- add option to disable auphonic
- make video renderer use auphonic
- add self-update to ins update
- init update command
- refactor packages

## [0.6.12](https://github.com/instantOS/instantCLI/compare/v0.6.11...v0.6.12) - 2025-11-19

### Other

- Migrate workflows to Blacksmith
- remove redundant compilation

## [0.6.11](https://github.com/instantOS/instantCLI/compare/v0.6.10...v0.6.11) - 2025-11-19

### Fixed

- *(ci)* use cargo-ndk for android termux builds to resolve linking issues

### Other

- make self hosted
- refactor launch handle function

## [0.6.10](https://github.com/instantOS/instantCLI/compare/v0.6.9...v0.6.10) - 2025-11-19

### Fixed

- fix android compilation

### Other

- Merge branch 'dev'

## [0.6.9](https://github.com/instantOS/instantCLI/compare/v0.6.8...v0.6.9) - 2025-11-18

### Other

- Configure release-plz to include changelog in GitHub releases

## [0.6.8](https://github.com/instantOS/instantCLI/compare/v0.6.7...v0.6.8) - 2025-11-18

### Other

- better release checking
- sync PKGBUILD version
- release v0.6.7
- add xdg utils requirement

## [0.6.7](https://github.com/instantOS/instantCLI/compare/v0.6.6...v0.6.7) - 2025-11-18

### Fixed

- fix legacy fzf
- fix ignoring

### Other

- no more fzf fuckery
- more fzf debugging
- better debugging
- document deps
- better fzf fallback

## [0.6.6](https://github.com/instantOS/instantCLI/compare/v0.6.5...v0.6.6) - 2025-11-17

### Fixed

- fix termux builds
- fix install script

### Other

- add shfmt

## [0.6.5](https://github.com/instantOS/instantCLI/compare/v0.6.4...v0.6.5) - 2025-11-17

### Other

- horrible awk code to find newest working release
- init ARM support

## [0.6.4](https://github.com/instantOS/instantCLI/compare/v0.6.3...v0.6.4) - 2025-11-17

### Other

- format
- repair debug
- format
- refactor command handling

## [0.6.3](https://github.com/instantOS/instantCLI/compare/v0.6.2...v0.6.3) - 2025-11-17

### Other

- update agents
- init ignore command
- refactor main function
- remove old help message
- add steamOS installer

## [0.6.2](https://github.com/instantOS/instantCLI/compare/v0.6.1...v0.6.2) - 2025-11-17

### Fixed

- fix self-update

### Other

- add exec command
- Release ins version 0.6.0
- sync PKGBUILD version
- release v0.5.6

## [0.6.1](https://github.com/instantOS/instantCLI/compare/v0.6.0...v0.6.1) - 2025-11-16

### Fixed

- fix self-update

## [0.5.6](https://github.com/instantOS/instantCLI/compare/v0.5.5...v0.5.6) - 2025-11-16

### Fixed

- fix install script on ubuntu

### Other

- compat with older fzf versions
- self update
- format and sudo
- format
- make deps cleaner
- init qr screenshotting
- add icons to assist help menu

## [0.5.5](https://github.com/instantOS/instantCLI/compare/v0.5.4...v0.5.5) - 2025-11-16

### Fixed

- fix invalid test

## [0.5.4](https://github.com/instantOS/instantCLI/compare/v0.5.3...v0.5.4) - 2025-11-16

### Fixed

- fix install script

### Other

- dynamic swaymsg reload
- switch out pactl with wpctl
- rename add to clone and make init more user friendly
- add area to pictures assist
- add notification utils
- add fullscreen assist
- add cmatrix assist
- add asciiquarium assist

## [0.5.3](https://github.com/instantOS/instantCLI/compare/v0.5.2...v0.5.3) - 2025-11-14

### Other

- improve install script
- init install script
- allow installing assist deps inside a terminal
- refactor dependency handling
- refactor slider assists
- init brightness assist
- add q to valid chord keys
- deduplicate screenshot logic
- add ocr assist

## [0.5.2](https://github.com/instantOS/instantCLI/compare/v0.5.1...v0.5.2) - 2025-11-13

### Fixed

- fix

### Other

- better key hints
- add key hints to sway
- add help message on sway
- unify dependencies and descriptions
- add password assist
- auphonic plans
- better help assist
- add help assist
- add assist sway setup command
- export instant assists as sway modes
- prevent TUI interference
- Revert "add exponential backoff"
- abstract away copying logic
- add exponential backoff
- add bruh moment
- better icons
- swap p and n assists
- format
- init flatpak dependency system
- dedup packages
- format
- add full screen screenshot assist
- abstract away area selection
- make imgur uploader more rusty
- refactor assist registry

## [0.5.1](https://github.com/instantOS/instantCLI/compare/v0.5.0...v0.5.1) - 2025-11-11

### Other

- screenshotpretty
- init sc assist
- add flameshot assist
- extract display server stuff
- remove unnecessary wrapper
- deduplicate terminal logic
- init more assists
- make assists tree-like
- implement playerctl assist
- init assist command

## [0.4.1](https://github.com/instantOS/instantCLI/compare/v0.4.0...v0.4.1) - 2025-11-10

### Other

- replace unsafe usage
- add menu fallback
- init fallback plans
- remove old function
- add chord support to menu server
- add chords ability from stdin
- some fixes
- init more generic chords
- init keychords

## [0.2.10](https://github.com/instantOS/instantCLI/compare/v0.2.9...v0.2.10) - 2025-11-03

### Other

- deduplicate single file logic
- init work on single file dependencies
- simplify restic handling

## [0.2.9](https://github.com/instantOS/instantCLI/compare/v0.2.8...v0.2.9) - 2025-11-02

### Fixed

- fix directory choosing for single file saves
- fix single file flow again
- fix more single file issues
- fix single file restore

### Other

- try another fix
- refactor setup
- add ability to cancel setup
- resolve single file conflicts

## [0.2.8](https://github.com/instantOS/instantCLI/compare/v0.2.7...v0.2.8) - 2025-11-01

### Other

- format

## [0.2.7](https://github.com/instantOS/instantCLI/compare/v0.2.6...v0.2.7) - 2025-10-29

### Fixed

- fix snapshot inference
- fix compilation
- fix non-existent thing
- fix typo
- fix prompts
- fix tests
- fix single file security
- fix single file backup
- fix compilation
- *(game)* picker scope for ins game deps

### Other

- validate usernames
- make system the single source of truth for user settings
- refactor users module
- unify launch settings
- prettier settings
- implement network settings
- init network settings
- rename more settings
- more friendly setting names
- refactor handle_settings
- better multiple choice settings
- better settings serialization
- init cockpit settings
- remove old fields
- add bug md
- init editor refactor
- refactor game install
- format
- refactor game display
- resolve game add details
- init game manager refactor
- init refactor
- more setup flow
- tests and further implementation
- init better setup
- add TODO comments
- more filegone
- init filegone
- filegone
- hacky single file fix
- make tests more silent
- debug
- init single file tests
- init supporting files as saves and deps
- fetchadd plans
- file support plans
- better deps flow

## [0.2.6](https://github.com/instantOS/instantCLI/compare/v0.2.5...v0.2.6) - 2025-10-17

### Fixed

- fix CI

## [0.2.5](https://github.com/instantOS/instantCLI/compare/v0.2.4...v0.2.5) - 2025-10-17

### Fixed

- fix typo

### Other

- yamlfmt

## [0.2.4](https://github.com/instantOS/instantCLI/compare/v0.2.3...v0.2.4) - 2025-10-17

### Fixed

- fix appimage build

## [0.2.3](https://github.com/instantOS/instantCLI/compare/v0.2.2...v0.2.3) - 2025-10-17

### Fixed

- fix appimage

### Other

- add appimage to CI
- change appimage icon
- simplify appimage
- init appimage build
- desus christ

## [0.2.2](https://github.com/instantOS/instantCLI/compare/v0.2.1...v0.2.2) - 2025-10-15

### Other

- init binstall compatibility
- add comment tests

## [0.2.1](https://github.com/instantOS/instantCLI/compare/v0.2.0...v0.2.1) - 2025-10-14

### Other

- refactor edit
- init edit implementation
- init edit command
- add cargo lock
- Merge branch 'dev' of github.com:instantOS/instantCLI into dev
- Merge branch 'dev'

## [0.1.13](https://github.com/instantOS/instantCLI/compare/v0.1.12...v0.1.13) - 2025-10-10

### Other

- format

## [0.1.12](https://github.com/instantOS/instantCLI/compare/v0.1.11...v0.1.12) - 2025-10-09

### Fixed

- fix sorting
- fix overlay not appearing
- fix some document stuff
- fix typo
- fix description
- fix titlecard stuff

### Other

- move css to own file
- do not make overlays transparent
- init new NLE pipeline
- init new NLE
- init video render dry run
- more wine plans
- downscale overlays
- init new stats command and prerendering
- init overlay support
- improve markdown card caching
- parse separators
- init titlecard command
- improve fetch pring

## [0.1.11](https://github.com/instantOS/instantCLI/compare/v0.1.10...v0.1.11) - 2025-10-07

### Added

- change mimetype sorting to prioritize commonly used ones

### Fixed

- fix title cards
- fix render format
- fix out file
- fix typo
- fix fzf command previews and adjust defaultapps preview
- fix stuff

### Other

- plan music
- init title card generator
- document state machine
- init rendering
- video render CLI
- init video document parsing
- ignore testing file
- add output args
- ignore webm
- init whisper transcription
- init video feature
- video plans
- init wineprefix plans
- add confirm dialog for adding game save paths
- refactor settings menu logic
- init icons for default settings
- make mime type settings faster
- init default apps setting
- make strings selectable
- add new plan
- remove
- remove further
- only dependency folders for now
- more dependency stuff
- deps display
- game deps CLI
- init deps
- ensure restic package on ins game commands

## [0.1.10](https://github.com/instantOS/instantCLI/compare/v0.1.9...v0.1.10) - 2025-10-03

### Added

- *(fix)* bump minor version hopefully

### Fixed

- fix CI
- better edit icon
- filepicker preselection
- fix fzf confirm colors
- fix icon
- fix nerd fonts
- fix conflict
- fix package command
- fix multi select
- fix snake case
- fix compilation
- fix password prompt
- fix release workflow

### Other

- add setup helpful message
- restructure setup
- refactor game setup
- better file picker handling of incorrect selections
- better setup process
- improve setup
- add easier setup for uninstalled games
- add path picker to game manager
- file path picker in add
- add placeholder notices
- make picker return a pathbuf
- refactor
- add hints to yazi picker
- add yazi picker
- rename menu wrapper
- init file picker plan
- remove configmanager wrapper
- update further
- update agents files
- finish toggle rework
- init better toggle icons
- init migration to own nerd fonts crate
- deduplicate, make more intuitive
- add direct settings access via CLI
- add upgrade settings entry
- add package installer
- add about section
- better settings preview
- better shell editing
- remove old code
- init user password management
- better group management
- massively simplify
- systemd root support
- migrate to systemd module
- add systemd management module
- add disk settings
- better settings requirements
- bluetooth plans
- what
- huh
- init bluetooth setting
- styling
- remove initialkey
- more settings value stuff
- init settings value preselection
- init optional preselection
- improve toggling
- change how toggling works
- change settings UI loop
- further refactor
- further refactor
- more refactor
- init settings menu refactor
- style
- add password to server
- implement password prompt
- add requirements prompt
- manageduser stuff
- init user editing mod
- init user settings
- init arch for applying settings
- more implementation for external stuff
- init requirements system
- better category preview
- more intuitive stuff
- add search and back and new plans
- init settings
- add old settings

## [0.1.10] - 2025-10-11

### Features

- **(video)** Add video editing commands
- **(completions)** Add shell completion commands

## [0.1.9] - 2025-09-26

### Fixed

- fix again, I am stupid
- fix?
- project needs cmake now
- fixes
- fix some stuff
- fix git error

### Other

- add restic to CI
- fix runuser
- init mise toml
- better path arg handling
- add json tests
- more json output
- format and more json output
- better output
- better repo output
- make compile again
- start manually iconning again
- faster test times
- refactor git
- more output fixes
- remove duplicate icons
- more nerd icons
- use nerd_fonts crate
- more json output
- add nf dep
- help
- more json output features
- init better ouput everywhere
- init better output
- init warp md
- update readme

## [0.1.8](https://github.com/instantOS/instantCLI/compare/v0.1.7...v0.1.8) - 2025-09-27

### Fixed

- fix permission error

## [0.1.7](https://github.com/instantOS/instantCLI/compare/v0.1.6...v0.1.7) - 2025-09-27

### Other

- use custom build action
- ignore better
- ignore build artifacts

## [0.1.6](https://github.com/instantOS/instantCLI/compare/v0.1.5...v0.1.6) - 2025-09-27

### Fixed

- fix missing

## [0.1.5](https://github.com/instantOS/instantCLI/compare/v0.1.4...v0.1.5) - 2025-09-27

### Fixed

- *(ci)* missing deps

### Other

- restore game upon setup if no game save present

## [0.1.4](https://github.com/instantOS/instantCLI/compare/v0.1.3...v0.1.4) - 2025-09-27

### Fixed

- *(CI)* syntax error

### Other

- add sync pkgbuild workflow

## [0.1.3](https://github.com/instantOS/instantCLI/compare/v0.1.2...v0.1.3) - 2025-09-27

### Fixed

- fix completions and releasing

## [0.1.2](https://github.com/instantOS/instantCLI/compare/v0.1.1...v0.1.2) - 2025-09-27

### Fixed

- fix workflows

### Other

- init release-plz
- better PKGBUILD
