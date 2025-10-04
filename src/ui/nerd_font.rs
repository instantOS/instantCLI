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
    Lock,
    Unlock,

    // Media and audio
    VolumeUp,
    VolumeDown,
    VolumeMute,
    Play,
    Pause,
    Stop,

    // Communication and network
    Bluetooth,
    Wifi,
    Globe,
    Link,
    ExternalLink,

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
            Self::Cross => '✗',       // fa-times
            Self::CrossCircle => '', // fa-times-circle
            Self::Warning => '',     // fa-exclamation-triangle
            Self::Info => 'ℹ',        // fa-info-circle
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
            Self::Desktop => '',  // fa-desktop
            Self::Terminal => '', // fa-terminal
            Self::Gear => '⚙',     // fa-gear
            Self::Wrench => '',   // fa-wrench
            Self::Bug => '',      // fa-bug
            Self::Key => '',      // fa-key
            Self::Lock => '',     // fa-lock
            Self::Unlock => '',   // fa-unlock

            // Media and audio
            Self::VolumeUp => '󰝝',   // fa-volume-up
            Self::VolumeDown => '󰝞', // fa-volume-down
            Self::VolumeMute => '', // fa-volume-mute
            Self::Play => '▶',       // fa-play
            Self::Pause => '⏸',      // fa-pause
            Self::Stop => '⏹',       // fa-stop

            // Communication and network
            Self::Bluetooth => '',    // fa-bluetooth
            Self::Wifi => '',         // fa-wifi
            Self::Globe => '',        // fa-globe
            Self::Link => '',         // fa-link
            Self::ExternalLink => '', // fa-external-link

            // User and social
            Self::User => '',      // fa-user
            Self::Users => '',     // fa-users
            Self::UserPlus => '',  // fa-user-plus
            Self::UserMinus => '', // fa-user-minus

            // Actions and controls
            Self::Plus => '+',   // fa-plus
            Self::Minus => '-',  // fa-minus
            Self::Edit => '',   // fa-edit
            Self::Trash => '🗑',  // fa-trash
            Self::Search => '', // fa-search
            Self::Filter => '', // fa-filter

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

            // Additional icons for better semantics
            Self::Users2 => '',    // fa-users (alternative)
            Self::Shield => '🛡',    // fa-shield
            Self::HardDrive => '󰋊', // fa-hdd
            Self::Server => '🖥',    // fa-server
            Self::Cpu => '',       // fa-microchip
            Self::Memory => '󰍛',    // fa-memory
            Self::Upgrade => '⬆',   // fa-arrow-circle-up
            Self::About => 'ℹ',     // fa-info-circle
            Self::Partition => '', // fa-partition

            // Toggle-specific semantic icons
            Self::Palette => '󰏘',   // fa-palette
            Self::Magic => '',     // fa-magic
            Self::Clipboard => '', // fa-clipboard

            // File type icons
            Self::Image => '🖼',      // fa-image
            Self::Video => '🎥',      // fa-video
            Self::Music => '🎵',      // fa-music
            Self::FilePdf => '📄',    // fa-file-pdf
            Self::FileWord => '📝',   // fa-file-word
            Self::FileExcel => '📊',  // fa-file-excel
            Self::FilePresentation => '📊', // fa-file-powerpoint
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
