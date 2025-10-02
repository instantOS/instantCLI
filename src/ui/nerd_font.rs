/// Custom NerdFont enum with carefully selected icons for InstantCLI
/// 
/// This replaces the nerd_fonts crate with a curated set of icons that are:
/// - More semantically appropriate for their usage
/// - Consistent in style
/// - Well-supported across nerd font implementations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NerdFont {
    // Navigation and UI
    ArrowLeft,      // 
    ArrowUp,        // 
    ArrowDown,      // 
    ArrowRight,     // 
    ChevronLeft,    // 
    ChevronRight,   // 
    ChevronUp,      // 
    ChevronDown,    // 
    
    // Status and feedback
    Check,          // 
    CheckCircle,    // 
    CheckSquare,    // 
    Cross,          // 
    CrossCircle,    // 
    Warning,        // 
    Info,           // 
    Question,       // 
    
    // Files and folders
    Folder,         // 
    FolderOpen,     // 
    File,           // 
    FileText,       // 
    Save,           // 
    Download,       // 
    Upload,         // 
    Archive,        // 
    
    // System and hardware
    Desktop,        // 
    Terminal,       // 
    Gear,           // 
    Wrench,         // 
    Bug,            // 
    Key,            // 
    Lock,           // 
    Unlock,         // 
    
    // Media and audio
    VolumeUp,       // 
    VolumeDown,     // 
    VolumeMute,     // 
    Play,           // 
    Pause,          // 
    Stop,           // 
    
    // Communication and network
    Bluetooth,      // 
    Wifi,           // 
    Globe,          // 
    Link,           // 
    ExternalLink,   // 
    
    // User and social
    User,           // 
    Users,          // 
    UserPlus,       // 
    UserMinus,      // 
    
    // Actions and controls
    Plus,           // 
    Minus,          // 
    Edit,           // 
    Trash,          // 
    Search,         // 
    Filter,         // 
    
    // Gaming and entertainment
    Gamepad,        // 
    Trophy,         // 
    Star,           // 
    Flag,           // 
    Target,         // 
    
    // Data and analytics
    Chart,          // 
    List,           // 
    Table,          // 
    Database,       // 
    
    // Time and scheduling
    Clock,          // 
    Calendar,       // 
    Timer,          // 
    
    // Development and tools
    Code,           // 
    Git,            // 
    Branch,         // 
    Tag,            // 
    Package,        // 
    
    // UI controls
    ToggleOn,       // 
    ToggleOff,      // 
    Square,         // 
    SquareCheck,    // 
    Circle,         // 
    CircleCheck,    // 
    
    // Miscellaneous
    Lightbulb,      //
    Rocket,         //
    Refresh,        //
    Sync,           //
    Home,           //
    Settings,       //

    // Additional icons for better semantics
    Users2,         //
    Shield,         //
    HardDrive,      //
    Server,         //
    Cpu,            //
    Memory,         //
    Upgrade,        //
    About,          //
    Partition,      //

    // Toggle-specific semantic icons
    Palette,        //
    Magic,          //
    Clipboard,      //
}

impl NerdFont {
    /// Get the Unicode character for this nerd font icon
    pub const fn unicode(&self) -> char {
        match self {
            // Navigation and UI
            Self::ArrowLeft => '\u{f060}',      // fa-arrow-left
            Self::ArrowUp => '\u{f062}',        // fa-arrow-up
            Self::ArrowDown => '\u{f063}',      // fa-arrow-down
            Self::ArrowRight => '\u{f061}',     // fa-arrow-right
            Self::ChevronLeft => '\u{f053}',    // fa-chevron-left
            Self::ChevronRight => '\u{f054}',   // fa-chevron-right
            Self::ChevronUp => '\u{f077}',      // fa-chevron-up
            Self::ChevronDown => '\u{f078}',    // fa-chevron-down
            
            // Status and feedback
            Self::Check => '\u{f00c}',          // fa-check
            Self::CheckCircle => '\u{f058}',    // fa-check-circle
            Self::CheckSquare => '\u{f14a}',    // fa-check-square
            Self::Cross => '\u{f00d}',          // fa-times
            Self::CrossCircle => '\u{f057}',    // fa-times-circle
            Self::Warning => '\u{f071}',        // fa-exclamation-triangle
            Self::Info => '\u{f05a}',           // fa-info-circle
            Self::Question => '\u{f059}',       // fa-question-circle
            
            // Files and folders
            Self::Folder => '\u{f07b}',         // fa-folder
            Self::FolderOpen => '\u{f07c}',     // fa-folder-open
            Self::File => '\u{f15b}',           // fa-file
            Self::FileText => '\u{f15c}',       // fa-file-text
            Self::Save => '\u{f0c7}',           // fa-save
            Self::Download => '\u{f019}',       // fa-download
            Self::Upload => '\u{f093}',         // fa-upload
            Self::Archive => '\u{f187}',        // fa-archive
            
            // System and hardware
            Self::Desktop => '\u{f108}',        // fa-desktop
            Self::Terminal => '\u{f120}',       // fa-terminal
            Self::Gear => '\u{f013}',           // fa-gear
            Self::Wrench => '\u{f0ad}',         // fa-wrench
            Self::Bug => '\u{f188}',            // fa-bug
            Self::Key => '\u{f084}',            // fa-key
            Self::Lock => '\u{f023}',           // fa-lock
            Self::Unlock => '\u{f09c}',         // fa-unlock
            
            // Media and audio
            Self::VolumeUp => '\u{f028}',       // fa-volume-up
            Self::VolumeDown => '\u{f027}',     // fa-volume-down
            Self::VolumeMute => '\u{f026}',     // fa-volume-mute
            Self::Play => '\u{f04b}',           // fa-play
            Self::Pause => '\u{f04c}',          // fa-pause
            Self::Stop => '\u{f04d}',           // fa-stop
            
            // Communication and network
            Self::Bluetooth => '\u{f293}',      // fa-bluetooth
            Self::Wifi => '\u{f1eb}',           // fa-wifi
            Self::Globe => '\u{f0ac}',          // fa-globe
            Self::Link => '\u{f0c1}',           // fa-link
            Self::ExternalLink => '\u{f08e}',   // fa-external-link
            
            // User and social
            Self::User => '\u{f007}',           // fa-user
            Self::Users => '\u{f0c0}',          // fa-users
            Self::UserPlus => '\u{f234}',       // fa-user-plus
            Self::UserMinus => '\u{f235}',      // fa-user-minus
            
            // Actions and controls
            Self::Plus => '\u{f067}',           // fa-plus
            Self::Minus => '\u{f068}',          // fa-minus
            Self::Edit => '\u{f044}',           // fa-edit
            Self::Trash => '\u{f1f8}',          // fa-trash
            Self::Search => '\u{f002}',         // fa-search
            Self::Filter => '\u{f0b0}',         // fa-filter
            
            // Gaming and entertainment
            Self::Gamepad => '\u{f11b}',        // fa-gamepad
            Self::Trophy => '\u{f091}',         // fa-trophy
            Self::Star => '\u{f005}',           // fa-star
            Self::Flag => '\u{f024}',           // fa-flag
            Self::Target => '\u{f140}',         // fa-bullseye
            
            // Data and analytics
            Self::Chart => '\u{f080}',          // fa-bar-chart
            Self::List => '\u{f03a}',           // fa-list
            Self::Table => '\u{f0ce}',          // fa-table
            Self::Database => '\u{f1c0}',       // fa-database
            
            // Time and scheduling
            Self::Clock => '\u{f017}',          // fa-clock
            Self::Calendar => '\u{f073}',       // fa-calendar
            Self::Timer => '\u{f2f2}',          // fa-stopwatch
            
            // Development and tools
            Self::Code => '\u{f121}',           // fa-code
            Self::Git => '\u{f1d3}',            // fa-git
            Self::Branch => '\u{f126}',         // fa-code-branch
            Self::Tag => '\u{f02b}',            // fa-tag
            Self::Package => '\u{f187}',        // fa-archive (reused)
            
            // UI controls
            Self::ToggleOn => '\u{f205}',       // fa-toggle-on
            Self::ToggleOff => '\u{f204}',      // fa-toggle-off
            Self::Square => '\u{f0c8}',         // fa-square
            Self::SquareCheck => '\u{f14a}',    // fa-check-square (reused)
            Self::Circle => '\u{f111}',         // fa-circle
            Self::CircleCheck => '\u{f058}',    // fa-check-circle (reused)
            
            // Miscellaneous
            Self::Lightbulb => '\u{f0eb}',      // fa-lightbulb
            Self::Rocket => '\u{f135}',         // fa-rocket
            Self::Refresh => '\u{f021}',        // fa-refresh
            Self::Sync => '\u{f021}',           // fa-refresh (reused)
            Self::Home => '\u{f015}',           // fa-home
            Self::Settings => '\u{f013}',       // fa-gear (reused)

            // Additional icons for better semantics
            Self::Users2 => '\u{f0c0}',         // fa-users (alternative)
            Self::Shield => '\u{f132}',         // fa-shield
            Self::HardDrive => '\u{f0a0}',      // fa-hdd
            Self::Server => '\u{f233}',         // fa-server
            Self::Cpu => '\u{f2db}',            // fa-microchip
            Self::Memory => '\u{f538}',         // fa-memory
            Self::Upgrade => '\u{f0aa}',        // fa-arrow-circle-up
            Self::About => '\u{f05a}',          // fa-info-circle (reused but semantic)
            Self::Partition => '\u{f1c0}',      // fa-database (reused but semantic)

            // Toggle-specific semantic icons
            Self::Palette => '\u{f53f}',        // fa-palette
            Self::Magic => '\u{f0d0}',          // fa-magic
            Self::Clipboard => '\u{f328}',      // fa-clipboard
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
