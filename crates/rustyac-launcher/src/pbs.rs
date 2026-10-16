// SPDX-License-Identifier: GPL-3.0-or-later

//! `--import-pbs`: the player's personal bests from an Assetto Corsa install. The theme's PB
//! manager reads `personalbest.ini` of the player's folder into its own storage at every start
//! and writes its list back there, so the import is that file: every combo of the other folder's
//! `personalbest.ini` whose time beats ours (or that we lack) is copied in. The other folder is
//! only read.

use crate::bridge::Thrown;
use crate::ini::IniFile;

/// Copies the better laps of `from` into `to`: (copied, ours kept as better or equal).
pub fn import(from: &str, to: &str) -> Result<(usize, usize), Thrown> {
    let (theirs, ours) = (IniFile::new(from), IniFile::new(to));
    let time = |f: &IniFile, combo: &str| f.read(Some(combo), Some("TIME")).parse::<i64>().ok().filter(|t| *t > 0);
    let (mut copied, mut kept) = (0, 0);
    for combo in theirs.categories()? {
        let Some(t) = time(&theirs, &combo) else { continue };
        if time(&ours, &combo).is_some_and(|o| o <= t) {
            kept += 1;
            continue;
        }
        ours.write(Some(&combo), Some("DATE"), Some(&theirs.read(Some(&combo), Some("DATE"))))?;
        ours.write(Some(&combo), Some("TIME"), Some(&t.to_string()))?;
        copied += 1;
    }
    Ok((copied, kept))
}
