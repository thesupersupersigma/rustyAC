// SPDX-License-Identifier: GPL-3.0-or-later
// check.mjs --script for browser.sh's Full-mode fallback case (Task W2): what the page's banner
// shows once a drive Full mode cannot draw (a track needing Custom Shaders Patch) has started.
const banner = document.getElementById('fallback');
return { shown: !banner.hidden, text: document.getElementById('fallback-text').textContent, same: window.rustyac.notes.includes(window.rustyac.fallback || '\u0000') };
