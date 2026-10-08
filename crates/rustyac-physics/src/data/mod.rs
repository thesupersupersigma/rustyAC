// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! Loading car data files into the physics parameter structs.
//!
//! A car's files are read the way the game reads them: out of the archive next to the data
//! folder (`content/cars/<car>/data.acd`, decrypted in memory by `rustyac_content::acd`) when
//! there is one, else from the plain folder (`data/` of an unpacked car, `cardata/<car>`).

pub mod ini;
pub mod tyres_ini;

use std::path::Path;

/// Is there such a car data file (in the archive when the folder has one)?
pub fn exists(file: &Path) -> bool {
    rustyac_content::acd::exists(file)
}

/// A car data file's bytes; `Ok(None)` when there is no such file.
pub fn read(file: &Path) -> Result<Option<Vec<u8>>, String> {
    rustyac_content::acd::read(file)
}
