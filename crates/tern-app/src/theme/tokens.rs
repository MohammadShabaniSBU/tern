//! Spacing, radii, type and fixed sizes (docs/designs/09 Design tokens).
#![allow(dead_code)] // the full token set is declared even where a view doesn't use it yet

// Spacing
pub const SPACE_2: f32 = 2.0;
pub const SPACE_4: f32 = 4.0;
pub const SPACE_6: f32 = 6.0;
pub const SPACE_8: f32 = 8.0;
pub const SPACE_10: f32 = 10.0;
pub const SPACE_12: f32 = 12.0;
pub const SPACE_16: f32 = 16.0;
pub const SPACE_20: f32 = 20.0;
pub const SPACE_24: f32 = 24.0;
pub const SPACE_32: f32 = 32.0;
pub const SPACE_48: f32 = 48.0;

// Radii
pub const RADIUS_SM: f32 = 4.0;
pub const RADIUS_MD: f32 = 6.0;
pub const RADIUS_LG: f32 = 8.0;
pub const RADIUS_XL: f32 = 10.0;
pub const RADIUS_FULL: f32 = 999.0;

// Type sizes (UI: Inter; mono: JetBrains Mono)
pub const TEXT_DISPLAY: f32 = 20.0;
pub const TEXT_TITLE: f32 = 18.0;
pub const TEXT_HEADING: f32 = 16.0;
pub const TEXT_LABEL: f32 = 14.0;
pub const TEXT_BODY: f32 = 13.0;
pub const TEXT_SMALL: f32 = 12.0;
pub const TEXT_CAPTION: f32 = 11.0;
pub const MONO_VALUE: f32 = 12.0;
pub const MONO_KBD: f32 = 11.0;
pub const MONO_TERM: f32 = 13.0;

// Fixed sizes
pub const ROW_H: f32 = 36.0;
pub const ROW_H_TWO_LINE: f32 = 44.0;
pub const PALETTE_ITEM_H: f32 = 36.0;
pub const INPUT_H: f32 = 32.0;
pub const INPUT_H_LG: f32 = 36.0;
pub const BUTTON_H: f32 = 32.0;
pub const BUTTON_H_SM: f32 = 28.0;
pub const TAB_BAR_H: f32 = 40.0;
pub const TAB_H: f32 = 34.0;
pub const STATUS_BAR_H: f32 = 28.0;
pub const SIDEBAR_W: f32 = 208.0;
pub const DETAIL_W: f32 = 352.0;
pub const MODAL_W: f32 = 600.0;
pub const CONFIRM_W: f32 = 440.0;
pub const PALETTE_W: f32 = 640.0;
pub const TOAST_W: f32 = 360.0;
pub const STATUS_DOT: f32 = 8.0;
pub const CHIP_DOT: f32 = 6.0;
pub const ICON: f32 = 16.0;
pub const ICON_SM: f32 = 12.0;
/// Fixed width of the mode pill so the breadcrumb never jumps.
pub const MODE_PILL_W: f32 = 92.0;
/// Label column in list rows.
pub const ROW_LABEL_W: f32 = 168.0;
pub const ROW_TIME_W: f32 = 64.0;
