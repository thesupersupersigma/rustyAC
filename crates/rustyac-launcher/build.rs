// SPDX-License-Identifier: GPL-3.0-or-later
include!("../../packaging/version_res.rs");

fn main() {
    version_res("rustyac-launcher", "rustyac-launcher.exe", "rustyAC launcher");
}
