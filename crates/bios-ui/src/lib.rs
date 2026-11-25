//! How Bios looks: the theme, the map and the sequence rows.
//!
//! This is the only library crate that may use egui. Geometry lives in `bios-layout` and is
//! tested there without a window; the code here turns that geometry into paint.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod acts_on;
pub mod app_layout;
pub mod ask;
pub mod binder_card;
pub mod build_console;
pub mod build_folder;
pub mod canvas_tools;
pub mod changes;
pub mod contents_panel;
pub mod cut_hover;
pub mod docs;
pub mod enzymes_menu;
pub mod enzymes_panel;
pub mod feature_card;
pub mod find_bar;
pub mod icon;
pub mod intro;
pub mod kind_icons;
pub mod lucide;
pub mod map;
pub mod map_tools;
pub mod mark;
pub mod navigator;
pub mod pane;
pub mod picture;
pub mod plasmid_title;
pub mod primer_card;
pub mod primitives;
pub mod projects_home;
pub mod quick_open;
pub mod rail_menu;
pub mod row_menu;
pub mod rows;
pub mod selection_readout;
pub mod session_nav;
pub mod settings;
pub mod side_panel;
pub mod sidebar;
pub mod svg;
pub mod tab_strip;
pub mod theme;
pub mod verify_align;
pub mod verify_board;
pub mod verify_front;
pub mod verify_kit;
pub mod verify_nav;
pub mod view_tab;
