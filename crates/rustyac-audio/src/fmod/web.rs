// SPDX-License-Identifier: GPL-3.0-or-later

//! The browser's FMOD: FMOD Studio 2.03 for HTML5 (Firelight's JavaScript and wasm, fetched
//! from the preview pack) behind the same functions as the DLLs. Every function of
//! [`super::raw`] reaches `globalThis.rustyacSound.call` (`web/sound.js`) with its name, its
//! arguments as 32-bit words and its one string; what FMOD gives back comes back as words. The
//! JavaScript side translates 1.08's calls into 2.03's, so the layer above and the call log are
//! the desktop's.
//!
//! The game's two DSP plug-ins are not registered in the browser: FMOD's HTML5 build cannot run
//! a JavaScript plug-in that a bank asks for (see `studio_register_plugin` in `sound.js`).

use std::any::TypeId;
use std::ffi::{c_char, c_void, CStr};

use wasm_bindgen::prelude::*;

use super::raw::{Handle, In, Out, OutHandle, Quiet, Sized, Str};
use super::types::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = rustyacSound, js_name = call)]
    // (the answer is a new array, the result first: a bank read back from this module can grow
    // its memory during the call, which leaves any view of the old memory empty)
    fn js_call(name: &str, text: &str, args: Vec<u32>) -> Vec<u32>;
    /// The calls kept back (`name\ttext\n` each; their words, each led by its count), then
    /// this call, in one crossing.
    #[wasm_bindgen(js_namespace = rustyacSound, js_name = batch)]
    fn js_batch(queued: &str, words: Vec<u32>, name: &str, text: &str, args: Vec<u32>) -> Vec<u32>;
}

/// Setters that give nothing back: they wait for the next call that does (one JavaScript
/// crossing for all of them; the frame's `studio_update` is such a call). They answer
/// `FMOD_OK` (`sound.js` warns in the console if one did not), or as `sound.js` answers a null
/// handle.
const KEPT_BACK: &[&str] = &["event_set_volume", "event_set_parameter_value", "event_set_3d_attributes", "event_set_pitch", "event_stop", "event_start", "event_set_timeline_position", "event_set_paused", "studio_set_listener_attributes"];
const FMOD_ERR_INVALID_HANDLE: i32 = 30;

thread_local! {
    static QUEUE: std::cell::RefCell<(String, Vec<u32>)> = Default::default();
}

/// One call's arguments on their way to JavaScript.
#[derive(Default)]
pub struct Words {
    args: Vec<u32>,
    text: String,
}

/// An argument as it crosses: what it puts in, what it takes back.
pub trait Wire {
    fn put(&self, _w: &mut Words) {}
    fn take(&self, _out: &mut std::slice::Iter<u32>) {}
}

impl Wire for i32 {
    fn put(&self, w: &mut Words) {
        w.args.push(*self as u32);
    }
}
impl Wire for u32 {
    fn put(&self, w: &mut Words) {
        w.args.push(*self);
    }
}
impl Wire for f32 {
    fn put(&self, w: &mut Words) {
        w.args.push(self.to_bits());
    }
}
impl Wire for bool {
    fn put(&self, w: &mut Words) {
        w.args.push(*self as u32);
    }
}
impl Wire for Handle {
    fn put(&self, w: &mut Words) {
        w.args.push(self.0 as usize as u32);
    }
}
impl Wire for Str {
    fn put(&self, w: &mut Words) {
        if !self.0.is_null() {
            // SAFETY: the API takes a terminated string here.
            w.text = unsafe { CStr::from_ptr(self.0) }.to_string_lossy().into_owned();
        }
    }
}

/// `T`'s bytes as little-endian words (zeros for a null pointer).
unsafe fn words_of(p: *const u8, size: usize, w: &mut Words) {
    for i in 0..size / 4 {
        w.args.push(if p.is_null() { 0 } else { p.add(i * 4).cast::<u32>().read_unaligned() });
    }
}

/// Writes `size` bytes of words to `p`.
unsafe fn write_words(p: *mut u8, size: usize, out: &mut std::slice::Iter<u32>) {
    for i in 0..size.div_ceil(4) {
        let word = out.next().copied().unwrap_or(0).to_le_bytes();
        if !p.is_null() {
            let n = (size - i * 4).min(4);
            std::ptr::copy_nonoverlapping(word.as_ptr(), p.add(i * 4), n);
        }
    }
}

impl<T: Pod> Wire for In<T> {
    fn put(&self, w: &mut Words) {
        // SAFETY: `T` is plain data the API reads at this address.
        unsafe { words_of(self.0.cast(), size_of::<T>(), w) }
    }
}
impl<T: Pod> Wire for Out<T> {
    fn take(&self, out: &mut std::slice::Iter<u32>) {
        // SAFETY: the caller's `T`, which FMOD writes.
        unsafe { write_words(self.0.cast(), size_of::<T>(), out) }
    }
}
impl Wire for OutHandle {
    fn take(&self, out: &mut std::slice::Iter<u32>) {
        let word = out.next().copied().unwrap_or(0);
        if !self.0.is_null() {
            // SAFETY: the caller's handle, which FMOD writes.
            unsafe { *self.0 = word as usize as RawHandle };
        }
    }
}
impl<T: 'static> Wire for Quiet<T> {
    fn take(&self, out: &mut std::slice::Iter<u32>) {
        let word = out.next().copied().unwrap_or(0);
        if self.0.is_null() {
            return;
        }
        // a number is written; a device name is empty; a callback or a structure is left alone
        // SAFETY: the caller's `T` (four bytes, or a character buffer).
        unsafe {
            if size_of::<T>() == 4 && TypeId::of::<T>() != TypeId::of::<c_void>() {
                self.0.cast::<u32>().write_unaligned(word);
            } else if TypeId::of::<T>() == TypeId::of::<c_char>() {
                self.0.cast::<u8>().write(0);
            }
        }
    }
}
impl Wire for Sized {
    fn put(&self, w: &mut Words) {
        if self.0.is_null() {
            return;
        }
        // SAFETY: the structure's first member is its size in bytes.
        unsafe { words_of(self.0.cast(), (*self.0.cast::<i32>()).clamp(0, 4096) as usize, w) }
    }
}
/// `Geometry::addPolygon`'s vertices: FMOD's HTML5 build has no `addPolygon` (the call fails).
impl Wire for *const Vector {}
/// `Studio::System::initialize`'s extra driver data: none in the browser.
impl Wire for *mut c_void {}
/// A plug-in's description: its address, for `sound_dsp_describe`.
impl Wire for *const c_void {
    fn put(&self, w: &mut Words) {
        w.args.push(*self as usize as u32);
    }
}
impl Wire for *mut i32 {
    fn take(&self, out: &mut std::slice::Iter<u32>) {
        // SAFETY: the caller's int, which FMOD writes.
        unsafe { write_words(self.cast(), 4, out) }
    }
}
impl Wire for *mut RawHandle {
    fn take(&self, out: &mut std::slice::Iter<u32>) {
        OutHandle(*self).take(out)
    }
}

/// One FMOD call through `sound.js`. What FMOD writes is written only when the call succeeded.
pub fn call(name: &str, args: &[&dyn Wire]) -> i32 {
    let mut w = Words::default();
    for arg in args {
        arg.put(&mut w);
    }
    if KEPT_BACK.contains(&name) {
        if w.args.first() == Some(&0) {
            return FMOD_ERR_INVALID_HANDLE;
        }
        QUEUE.with(|q| {
            let (calls, words) = &mut *q.borrow_mut();
            calls.push_str(name);
            calls.push('\t');
            calls.push_str(&w.text);
            calls.push('\n');
            words.push(w.args.len() as u32);
            words.extend(&w.args);
        });
        return FMOD_OK;
    }
    let out = if name == "system_get_geometry_occlusion" {
        // nothing occludes: FMOD's HTML5 build has no addPolygon, so no geometry has a polygon
        vec![FMOD_OK as u32, 0, 0]
    } else {
        let (calls, words) = QUEUE.with(|q| std::mem::take(&mut *q.borrow_mut()));
        if calls.is_empty() { js_call(name, &w.text, w.args) } else { js_batch(&calls, words, name, &w.text, w.args) }
    };
    let result = out.first().map_or(-1, |&r| r as i32);
    if result == FMOD_OK {
        let mut words = out[1..].iter();
        for arg in args {
            arg.take(&mut words);
        }
    }
    result
}

/// A file of the game's folder as the page has it (a bank, for FMOD's own file system).
#[wasm_bindgen]
pub fn sound_read_file(path: &str) -> Option<Vec<u8>> {
    rustyac_content::vfs::read(std::path::Path::new(path)).ok()
}
