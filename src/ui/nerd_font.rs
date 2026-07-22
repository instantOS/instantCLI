/// Custom NerdFont enum with carefully selected icons for InstantCLI
///
/// This replaces the nerd_fonts crate with a curated set of icons that are:
/// - More semantically appropriate for their usage
/// - Consistent in style
/// - Well-supported across nerd font implementations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NerdFont {
    // Navigation and UI
    ArrowLeft,
    ArrowUp,
    ArrowDown,
    ArrowRight,
    ChevronLeft,
    ChevronRight,
    ChevronUp,
    ChevronDown,

    // Status and feedback
    Check,
    CheckCircle,
    CheckSquare,
    CheckDouble,
    Cross,
    CrossCircle,
    Warning,
    Info,
    Question,

    // Files and folders
    Folder,
    FolderOpen,
    File,
    FileText,
    Save,
    Download,
    Upload,
    Archive,

    // System and hardware
    Desktop,
    Terminal,
    Gear,
    Wrench,
    Bug,
    Key,
    Keyboard,
    Lock,
    ClosedLockWithKey,
    Unlock,
    PowerOff,
    Reboot,
    Continue,

    // Media and audio
    VolumeUp,
    VolumeDown,
    VolumeMute,
    Play,
    PlayCircle,
    Pause,
    PauseCircle,
    Stop,

    // Communication and network
    Bluetooth,
    Wifi,
    Globe,
    Language,
    Link,
    ExternalLink,
    Bell,
    BellSlash,
    Envelope,
    EnvelopeOpen,

    // User and social
    User,
    Users,
    UserPlus,
    UserMinus,

    // Actions and controls
    Plus,
    Minus,
    Edit,
    Trash,
    Search,
    Filter,
    QrCode,
    Smile,

    // Gaming and entertainment
    Gamepad,
    Trophy,
    Star,
    Flag,
    Target,

    // Data and analytics
    Chart,
    List,
    Table,
    Database,

    // Time and scheduling
    Clock,
    Calendar,
    Timer,

    // Development and tools
    Code,
    Git,
    Branch,
    Tag,
    Package,

    // UI controls
    ToggleOn,
    ToggleOff,
    Square,
    SquareCheck,
    Circle,
    CircleCheck,

    // Miscellaneous
    Lightbulb,
    Rocket,
    Refresh,
    Sync,
    Home,
    Settings,
    Wine,
    Moon,

    // Additional icons for better semantics
    Users2,
    Shield,
    HardDrive,
    Server,
    Cpu,
    Memory,
    Upgrade,
    About,
    Partition,
    Printer,

    // Toggle-specific semantic icons
    Palette,
    Magic,
    Clipboard,

    // File type icons
    Image,
    Video,
    Music,
    FilePdf,
    FileWord,
    FileExcel,
    FilePresentation,

    // Version Control and Development
    GitCommit,
    GitMerge,
    GitBranch,
    GitPullRequest,
    GitCompare,
    FileCode,
    FileConfig,

    // Status and Operations
    Clock2,
    Sync2,
    Cloud2,
    CloudDownload,
    CloudUpload,
    BackupRestore,
    Database2,

    // System and Performance
    Monitor,
    Shield2,
    Network,
    Server2,
    TerminalBash,
    TerminalPowershell,
    TerminalUbuntu,
    Activity,

    // Gaming Enhancement
    Controller,
    Joystick,
    Achievement,
    HeartFilled,

    // File and Folder Variants
    FolderConfig,
    FolderGit,
    FolderActive,
    FileBinary,
    FileSymlink,

    // Operations and Actions
    Debug,
    SettingsGear,
    Broom,
    Sliders,
    Help,
    InfoCircle,

    // Application and Workspace
    Workspace,
    WorkspaceTrusted,
    WorkspaceUntrusted,
    RootFolder,
    FolderLibrary,

    // Dotfile Specific
    DotFile,
    Hash,
    SourceBranch,
    SourceMerge,
    SourceCommit,

    // Cloud and Backup
    CloudSync,
    CloudCheck,
    CloudAlert,
    CloudLock,

    // Performance and Monitoring
    Performance,
    Tachograph,
    ActivityMonitor,
    Heartbeat,

    // Security and Privacy
    ShieldCheck,
    ShieldLock,
    ShieldAlert,
    ShieldBug,

    // Testing and Development
    TestTube,
    Flask,
    BugReport,
    CodeReview,

    // Hardware and Graphics
    Gpu,
    MemoryModule,
    Ssd,
    HardDisk,
    Fan,

    // Input Devices
    MousePointer,
    Mouse,

    // Food and Beverages
    Coffee,

    // Faces and Emotions
    Frown,
    FishBowl,
    Waves,

    // Text formatting and lists
    Bullet,
    ArrowSubItem,
    ArrowPointer,

    // Boot and Firmware
    Efi,

    // Recording
    CircleStop,

    // Weather and Temperature
    Snowflake,

    // Gaming and Emulation
    Disc,
    Windows,
    Fullscreen,
    Fish,
    Steam,
}

impl NerdFont {
    /// Get the Unicode character for this nerd font icon
    pub const fn unicode(&self) -> char {
        match self {
            // Navigation and UI
            Self::ArrowLeft => '',    // fa-arrow-left
            Self::ArrowUp => '',      // fa-arrow-up
            Self::ArrowDown => '',    // fa-arrow-down
            Self::ArrowRight => '',   // fa-arrow-right
            Self::ChevronLeft => '',  // fa-chevron-left
            Self::ChevronRight => '', // fa-chevron-right
            Self::ChevronUp => '',    // fa-chevron-up
            Self::ChevronDown => '',  // fa-chevron-down

            // Status and feedback
            Self::Check => '✓',       // fa-check
            Self::CheckCircle => '', // fa-check-circle
            Self::CheckSquare => '', // fa-check-square
            Self::CheckDouble => '\u{ebd8}', // cod-check-all
            Self::Cross => '✗',       // fa-times
            Self::CrossCircle => '', // fa-times-circle
            Self::Warning => '',     // fa-exclamation-triangle
            Self::Info => '\u{f05a}', // fa-info-circle
            Self::Question => '',    // fa-question-circle

            // Files and folders
            Self::Folder => '',     // fa-folder
            Self::FolderOpen => '', // fa-folder-open
            Self::File => '',       // fa-file
            Self::FileText => '',   // fa-file-text
            Self::Save => '',       // fa-save
            Self::Download => '',   // fa-download
            Self::Upload => '',     // fa-upload
            Self::Archive => '',    // fa-archive

            // System and hardware
            Self::Desktop => '',           // fa-desktop
            Self::Terminal => '',          // fa-terminal
            Self::Gear => '\u{f013}',       // fa-gear
            Self::Wrench => '',            // fa-wrench
            Self::Bug => '',               // fa-bug
            Self::Key => '',               // fa-key
            Self::Keyboard => '\u{f11c}',   // U+2328 KEYBOARD / fa-keyboard-o
            Self::Lock => '',              // fa-lock
            Self::ClosedLockWithKey => '', // fa-lock-with-key
            Self::Unlock => '',            // fa-unlock
            Self::PowerOff => '',          // fa-power-off
            Self::Reboot => '',            // fa-repeat
            Self::Continue => '',          // fa-play

            // Media and audio
            Self::VolumeUp => '󰝝',     // fa-volume-up
            Self::VolumeDown => '󰝞',   // fa-volume-down
            Self::VolumeMute => '',   // fa-volume-mute
            Self::Play => '\u{f04b}',  // fa-play
            Self::PlayCircle => '',   // fa-play-circle
            Self::Pause => '\u{f04c}', // fa-pause
            Self::PauseCircle => '',  // fa-pause-circle
            Self::Stop => '\u{f04d}',  // fa-stop

            // Communication and network
            Self::Bluetooth => '',    // fa-bluetooth
            Self::Wifi => '',         // fa-wifi
            Self::Globe => '',        // fa-globe
            Self::Language => '',     // fa-language
            Self::Link => '',         // fa-link
            Self::ExternalLink => '', // fa-external-link
            Self::Bell => '',        // fa-bell
            Self::BellSlash => '',   // fa-bell-slash
            Self::Envelope => '',    // fa-envelope
            Self::EnvelopeOpen => '',// fa-envelope-open

            // User and social
            Self::User => '',      // fa-user
            Self::Users => '',     // fa-users
            Self::UserPlus => '',  // fa-user-plus
            Self::UserMinus => '', // fa-user-minus

            // Actions and controls
            Self::Plus => '+',          // fa-plus
            Self::Minus => '-',         // fa-minus
            Self::Edit => '',          // fa-edit
            Self::Trash => '\u{f1f8}',  // fa-trash
            Self::Search => '',        // fa-search
            Self::Filter => '',        // fa-filter
            Self::QrCode => '\u{f029}', // fa-qrcode
            Self::Smile => '\u{f118}',  // fa-smile-o

            // Gaming and entertainment
            Self::Gamepad => '', // fa-gamepad
            Self::Trophy => '',  // fa-trophy
            Self::Star => '',    // fa-star
            Self::Flag => '🏳',    // fa-flag
            Self::Target => '',  // fa-bullseye

            // Data and analytics
            Self::Chart => '',    // fa-bar-chart
            Self::List => '',     // fa-list
            Self::Table => '',    // fa-table
            Self::Database => '', // fa-database

            // Time and scheduling
            Self::Clock => '',    // fa-clock
            Self::Calendar => '󰃭', // fa-calendar
            Self::Timer => '⏱',    // fa-stopwatch

            // Development and tools
            Self::Code => '',    // fa-code
            Self::Git => '',     // fa-git
            Self::Branch => '',  // fa-code-branch
            Self::Tag => '',     // fa-tag
            Self::Package => '', // fa-package

            // UI controls
            Self::ToggleOn => '',    // fa-toggle-on
            Self::ToggleOff => '',   // fa-toggle-off
            Self::Square => '◻',      // fa-square
            Self::SquareCheck => '☑', // fa-check-square
            Self::Circle => '',      // fa-circle
            Self::CircleCheck => '', // fa-check-circle

            // Miscellaneous
            Self::Lightbulb => '', // fa-lightbulb
            Self::Rocket => '',    // fa-rocket
            Self::Refresh => '',   // fa-refresh
            Self::Sync => '',      // fa-sync
            Self::Home => '',      // fa-home
            Self::Settings => '',  // fa-settings
            Self::Wine => '󰡶',      // fa-wine
            Self::Moon => '',      // fa-moon

            // Additional icons for better semantics
            Self::Users2 => '',         // fa-users (alternative)
            Self::Shield => '\u{f132}',  // fa-shield
            Self::HardDrive => '󰋊',      // fa-hdd
            Self::Server => '\u{f233}',  // fa-server
            Self::Cpu => '',            // fa-microchip
            Self::Memory => '󰍛',         // fa-memory
            Self::Upgrade => '\u{f0aa}', // fa-arrow-circle-up
            Self::About => '\u{f05a}',   // fa-info-circle
            Self::Partition => '',      // fa-partition
            Self::Printer => '',        // fa-print

            // Toggle-specific semantic icons
            Self::Palette => '󰏘',   // fa-palette
            Self::Magic => '',     // fa-magic
            Self::Clipboard => '', // fa-clipboard

            // File type icons
            Self::Image => '\u{f03e}',     // fa-image
            Self::Video => '',            // fa-video
            Self::Music => '',            // fa-music
            Self::FilePdf => '',          // fa-file-pdf
            Self::FileWord => '',         // fa-file-word
            Self::FileExcel => '',        // fa-file-excel
            Self::FilePresentation => '󰐩', // fa-file-powerpoint

            // Version Control and Development
            Self::GitCommit => '',      // cod-git-commit
            Self::GitMerge => '',       // cod-git-merge
            Self::GitBranch => '',      // pl-branch (traditional git branch)
            Self::GitPullRequest => '', // cod-git-pull-request
            Self::GitCompare => '',     // cod-git-compare
            Self::FileCode => '',       // cod-file-code
            Self::FileConfig => '',     // seti-config

            // Status and Operations
            Self::Clock2 => '',        // fa-clock
            Self::Sync2 => '󰓦',         // md-sync
            Self::Cloud2 => '󰅟',        // md-cloud
            Self::CloudDownload => '', // fa-cloud-arrow-down
            Self::CloudUpload => '',   // fa-cloud-arrow-up
            Self::BackupRestore => '󰁯', // md-backup-restore
            Self::Database2 => '',     // fa-database

            // System and Performance
            Self::Monitor => '󰍹',            // md-monitor
            Self::Shield2 => '',            // fa-shield
            Self::Network => '󰛳',            // md-network
            Self::Server2 => '󰒋',            // md-server
            Self::TerminalBash => '',       // cod-terminal-bash
            Self::TerminalPowershell => '', // cod-terminal-powershell
            Self::TerminalUbuntu => '',     // cod-terminal-ubuntu
            Self::Activity => '',           // fa-heartbeat

            // Gaming Enhancement
            Self::Controller => '󰮂',  // md-controller-classic
            Self::Joystick => '',    // fa-playstation
            Self::Achievement => '', // fa-trophy
            Self::HeartFilled => '', // fa-heart

            // File and Folder Variants
            Self::FolderConfig => '', // custom-folder-config
            Self::FolderGit => '',    // custom-folder-git
            Self::FolderActive => '', // cod-folder-active
            Self::FileBinary => '',   // cod-file-binary
            Self::FileSymlink => '',  // cod-file-symlink-file

            // Operations and Actions
            Self::Debug => '',        // cod-debug
            Self::SettingsGear => '', // cod-settings-gear
            Self::Broom => '󰃢',        // md-broom
            Self::Sliders => '',      // fa-sliders
            Self::Help => '󰋖',         // md-help
            Self::InfoCircle => '',   // fa-circle-info

            // Application and Workspace
            Self::Workspace => '',          // cod-workspace-unknown
            Self::WorkspaceTrusted => '',   // cod-workspace-trusted
            Self::WorkspaceUntrusted => '', // cod-workspace-untrusted
            Self::RootFolder => '',         // cod-root-folder
            Self::FolderLibrary => '',      // cod-folder-library

            // Dotfile Specific
            Self::DotFile => '',      // oct-dot
            Self::Hash => '',         // oct-hash
            Self::SourceBranch => '󰘬', // md-source-branch
            Self::SourceMerge => '󰘭',  // md-source-merge
            Self::SourceCommit => '󰜘', // md-source-commit

            // Cloud and Backup
            Self::CloudSync => '󰘿',  // md-cloud-sync
            Self::CloudCheck => '󰅠', // md-cloud-check
            Self::CloudAlert => '󰧠', // md-cloud-alert
            Self::CloudLock => '󱇱',  // md-cloud-lock

            // Performance and Monitoring
            Self::Performance => '',     // fa-tachograph-digital
            Self::Tachograph => '',      // fa-heartbeat (performance monitoring)
            Self::ActivityMonitor => '󰦖', // md-progress-clock
            Self::Heartbeat => '󰗶',       // md-heart-pulse

            // Security and Privacy
            Self::ShieldCheck => '', // oct-shield-check
            Self::ShieldLock => '',  // oct-shield-lock
            Self::ShieldAlert => '󰻌', // md-shield-alert
            Self::ShieldBug => '󱏚',   // md-shield-bug

            // Testing and Development
            Self::TestTube => '󰙨',   // md-test-tube
            Self::Flask => '󰂓',      // md-flask
            Self::BugReport => '',  // cod-bug
            Self::CodeReview => '', // cod-code

            // Hardware and Graphics
            Self::Gpu => '󰢮',          // md-expansion-card
            Self::MemoryModule => '󰍛', // md-memory
            Self::Ssd => '󰋊',          // md-ssd
            Self::HardDisk => '󰋊',     // md-harddisk
            Self::Fan => '󰈐',          // md-fan

            // Input Devices
            Self::MousePointer => '', // fa-mouse-pointer
            Self::Mouse => '󰍽',        // md-mouse

            // Food and Beverages
            Self::Coffee => '', // fa-coffee

            // Faces and Emotions
            Self::Frown => '',    // md-emoticon-frown
            Self::FishBowl => '󰻳', // md-fishbowl
            Self::Waves => '󰞍',    // md-waves

            // Text formatting and lists
            Self::Bullet => '•',       // bullet point
            Self::ArrowSubItem => '↳', // arrow hook (sub-item indicator)
            Self::ArrowPointer => '→', // arrow pointer

            // Boot and Firmware
            Self::Efi => '󰒘', // cod-circuit-board (UEFI/firmware icon)

            // Recording
            Self::CircleStop => '\u{f28d}', // fa-stop-circle (stop recording)

            // Weather and Temperature
            Self::Snowflake => '󰼶', // md-snowflake (freeze)

            // Gaming and Emulation
            Self::Disc => '󰗮',              // md-disc (CD/DVD disc)
            Self::Windows => '\u{f17a}',    // fa-windows
            Self::Fullscreen => '\u{f065}', // fa-expand (fullscreen)
            Self::Fish => '󰈺',              // md-fish (dolphin)
            Self::Steam => '\u{f1b6}',      // fa-steam
        }
    }
}

impl std::fmt::Display for NerdFont {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.unicode())
    }
}

impl From<NerdFont> for char {
    fn from(icon: NerdFont) -> Self {
        icon.unicode()
    }
}

impl From<NerdFont> for String {
    fn from(icon: NerdFont) -> Self {
        icon.unicode().to_string()
    }
}
