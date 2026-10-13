// SPDX-License-Identifier: GPL-3.0-or-later

//! Assetto Corsa's widget library (`ksGUI.lib`, namespace `ksgui`), the form layout file and
//! the HUD objects, ported 1:1 from `acs.exe`. Nothing here knows about Direct3D, windows or
//! files: drawing goes through [`painter::Painter`] and the ini files are handed in.

// The ported code keeps the game's operations as the machine code has them: comparisons are
// spelled so that a NaN takes the game's branch.
#![allow(clippy::neg_cmp_op_on_partial_ord, clippy::too_many_arguments, clippy::nonminimal_bool, clippy::type_complexity, clippy::assign_op_pattern, clippy::manual_range_contains, clippy::needless_range_loop, clippy::manual_clamp, clippy::neg_multiply, clippy::collapsible_match)]

pub mod acos;
pub mod control;
pub mod end_session;
pub mod font;
pub mod form;
pub mod gui;
pub mod hud;
pub mod hud_cars;
pub mod hud_flags;
pub mod leaderboard;
pub mod menus;
pub mod overlay_leaderboard;
pub mod painter;
pub mod pit_menu;
pub mod spinner;
pub mod taskbar;
pub mod ttf;
pub mod widgets;
pub mod widgets2;
pub mod widgets3;
