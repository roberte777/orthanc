/// Netflix-inspired design tokens, shared between web and mobile UIs.
///
/// Each UI crate references these constants from its own CSS / inline styles.
/// Keeping them as Rust constants (rather than a single shared CSS file) lets
/// us tweak both UIs in lockstep with one edit and survives platform-specific
/// asset bundling differences.
pub mod colors {
    pub const BG: &str = "#141414";
    pub const BG_CARD: &str = "#1a1a1a";
    pub const BG_ELEVATED: &str = "#222222";
    pub const ACCENT: &str = "#e50914";
    pub const ACCENT_HOVER: &str = "#f6121d";
    pub const SUCCESS: &str = "#46d369";
    pub const DANGER: &str = "#e50914";
    pub const TEXT_PRIMARY: &str = "#ffffff";
    pub const TEXT_SECONDARY: &str = "#b3b3b3";
    pub const TEXT_MUTED: &str = "#737373";
    pub const BORDER: &str = "#2a2a2a";
}

pub mod spacing {
    pub const NAVBAR_HEIGHT_PX: u32 = 68;
    pub const BOTTOM_NAV_HEIGHT_PX: u32 = 64;
}
