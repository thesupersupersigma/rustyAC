// SPDX-License-Identifier: GPL-3.0-or-later

//! Assetto Corsa's widget library (`ksGUI.lib`, namespace `ksgui`), the form layout file and
//! the HUD objects, ported 1:1 from `acs.exe`. Nothing here knows about Direct3D, windows or
//! files: drawing goes through [`painter::Painter`] and the ini files are handed in.

// The ported code keeps the game's operations as the machine code has them: comparisons are
// spelled so that a NaN takes the game's branch.
#![allow(clippy::neg_cmp_op_on_partial_ord, clippy::too_many_arguments, clippy::nonminimal_bool, clippy::type_complexity, clippy::assign_op_pattern, clippy::manual_range_contains, clippy::needless_range_loop, clippy::manual_clamp, clippy::neg_multiply, clippy::collapsible_match)]

pub mod acos;
pub mod app_delta;
pub mod app_essentials;
pub mod app_eyes;
pub mod app_ffb;
pub mod app_friends;
pub mod app_gears;
pub mod app_laps;
pub mod app_mgu;
pub mod app_pedals;
pub mod app_position;
pub mod app_realtime;
pub mod app_small;
pub mod app_track_map;
pub mod app_tyres;
pub mod app_volumes;
pub mod apps;
pub mod control;
pub mod end_session;
pub mod esc_menu;
pub mod font;
pub mod form;
pub mod gui;
pub mod hud;
pub mod hud_cars;
pub mod hud_flags;
pub mod leaderboard;
pub mod locale;
pub mod menus;
pub mod online_hud;
pub mod overlay_leaderboard;
pub mod painter;
pub mod pit_menu;
pub mod replay_interface;
pub mod setup_car;
pub mod setup_electronics;
pub mod setup_fuel;
pub mod setup_gears;
pub mod setup_info;
pub mod setup_io;
pub mod setup_presets;
pub mod setup_screen;
pub mod setup_tab;
pub mod setup_tyres;
pub mod spinner;
pub mod taskbar;
pub mod time_table;
pub mod ttf;
pub mod widgets;
pub mod widgets2;
pub mod widgets3;
