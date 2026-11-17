//! The icons the UI uses, by what they mean here. They are glyphs of the Phosphor icon font
//! (regular weight), which [`super::theme::apply`] installs next to the text font, so an icon
//! can be written inside any label: `format!("{} Add sounds", icons::ADD)`.

use egui_phosphor::regular as ph;

// Messages
pub const INFO: &str = ph::INFO;
pub const CHECK_CIRCLE: &str = ph::CHECK_CIRCLE;
pub const WARNING: &str = ph::WARNING;
pub const ERROR: &str = ph::X_CIRCLE;
pub const CLOSE: &str = ph::X;

// What the cards are about
pub const MIC: &str = ph::MICROPHONE;
pub const VIRTUAL_MIC: &str = ph::PLUGS_CONNECTED;
pub const HEADPHONES: &str = ph::HEADPHONES;
pub const LATENCY: &str = ph::TIMER;
pub const WINDOW: &str = ph::APP_WINDOW;
pub const KEYBOARD: &str = ph::KEYBOARD;
pub const FEEDBACK: &str = ph::BELL;

// The header
pub const LOGO: &str = ph::WAVEFORM;
pub const MIC_OFF: &str = ph::MICROPHONE_SLASH;
pub const SAFE: &str = ph::SHIELD_CHECK;
pub const RESTORE: &str = ph::ARROW_COUNTER_CLOCKWISE;

// The navigation rail
pub const VOICE: &str = ph::USER_SOUND;
pub const SOUNDBOARD: &str = ph::SQUARES_FOUR;
pub const AUDIO: &str = ph::SLIDERS_HORIZONTAL;
pub const SETTINGS: &str = ph::GEAR_SIX;
pub const NAV_COLLAPSE: &str = ph::SIDEBAR_SIMPLE;

// Actions
pub const MORE: &str = ph::DOTS_THREE;
pub const ADD: &str = ph::PLUS;
pub const EDIT: &str = ph::PENCIL_SIMPLE;
pub const SAVE: &str = ph::FLOPPY_DISK;
pub const DELETE: &str = ph::TRASH;
pub const SHARE: &str = ph::EXPORT;
pub const IMPORT: &str = ph::DOWNLOAD_SIMPLE;
pub const EXPAND: &str = ph::CARET_DOWN;
pub const COLLAPSE: &str = ph::CARET_UP;
pub const PLAY: &str = ph::PLAY;
pub const STOP: &str = ph::STOP;
pub const NEXT: &str = ph::SKIP_FORWARD;

// The Voice page
pub const PRESETS: &str = ph::SPARKLE;
pub const MELODY: &str = ph::MUSIC_NOTES;
pub const FX_CHAIN: &str = ph::FADERS_HORIZONTAL;
pub const FX_GAIN: &str = ph::FADERS;
pub const FX_GATE: &str = ph::FUNNEL;
pub const FX_PITCH: &str = ph::ARROWS_OUT_LINE_VERTICAL;
pub const FX_TONE: &str = ph::EQUALIZER;
pub const FX_DISTORTION: &str = ph::LIGHTNING;
pub const FX_ROBOT: &str = ph::ROBOT;
pub const FX_CHORUS: &str = ph::USERS_THREE;
pub const FX_ECHO: &str = ph::REPEAT;
pub const FX_REVERB: &str = ph::CUBE;

// The Soundboard page
pub const SEARCH: &str = ph::MAGNIFYING_GLASS;
pub const VIEW_GRID: &str = ph::GRID_FOUR;
pub const VIEW_LIST: &str = ph::LIST;
pub const FOLDER: &str = ph::FOLDER;
pub const FOLDER_ADD: &str = ph::FOLDER_PLUS;
pub const LOCATE: &str = ph::FOLDER_OPEN;
pub const DROP_FILES: &str = ph::FILE_AUDIO;
pub const VOLUME: &str = ph::SPEAKER_HIGH;
pub const PAD_SIZE: &str = ph::ARROWS_OUT_SIMPLE;
pub const COLOR: &str = ph::PALETTE;
pub const PLAY_MODE: &str = ph::REPEAT_ONCE;

// Audio setup, Hotkeys and Settings
pub const USERS: &str = ph::USERS;
pub const SAFETY: &str = ph::SIREN;
pub const APPEARANCE: &str = ph::PAINT_BRUSH;
pub const FILE: &str = ph::FILE_TEXT;
pub const ABOUT: &str = ph::INFO;
pub const LINK: &str = ph::ARROW_SQUARE_OUT;
pub const HELP: &str = ph::QUESTION;
pub const RECORD: &str = ph::RECORD;

/// Every icon above, for the test that checks the font has them all.
#[cfg(test)]
pub const ALL: [&str; 64] = [
    HELP,
    RECORD,
    USERS,
    SAFETY,
    APPEARANCE,
    FILE,
    ABOUT,
    LINK,
    SEARCH,
    VIEW_GRID,
    VIEW_LIST,
    FOLDER,
    FOLDER_ADD,
    LOCATE,
    DROP_FILES,
    VOLUME,
    PAD_SIZE,
    COLOR,
    PLAY_MODE,
    MORE,
    ADD,
    EDIT,
    SAVE,
    DELETE,
    SHARE,
    IMPORT,
    EXPAND,
    COLLAPSE,
    PLAY,
    STOP,
    NEXT,
    PRESETS,
    MELODY,
    FX_CHAIN,
    FX_GAIN,
    FX_GATE,
    FX_PITCH,
    FX_TONE,
    FX_DISTORTION,
    FX_ROBOT,
    FX_CHORUS,
    FX_ECHO,
    FX_REVERB,
    INFO,
    CHECK_CIRCLE,
    WARNING,
    ERROR,
    CLOSE,
    MIC,
    VIRTUAL_MIC,
    HEADPHONES,
    LATENCY,
    WINDOW,
    KEYBOARD,
    FEEDBACK,
    LOGO,
    MIC_OFF,
    SAFE,
    RESTORE,
    VOICE,
    SOUNDBOARD,
    AUDIO,
    SETTINGS,
    NAV_COLLAPSE,
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Every icon must exist in the installed fonts (a missing glyph shows as an empty box).
    #[test]
    fn the_icon_font_has_every_icon() {
        let ctx = egui::Context::default();
        crate::ui::theme::apply(&ctx, crate::ui::theme::ThemeChoice::Dark);
        ctx.run_ui(egui::RawInput::default(), |_| {})
            .textures_delta
            .clear();
        let font = egui::FontId::proportional(14.0);
        for icon in ALL {
            let c = icon.chars().next().unwrap();
            assert!(
                ctx.fonts_mut(|f| f.has_glyph(&font, c)),
                "no glyph for U+{:04X}",
                c as u32
            );
        }
    }
}
