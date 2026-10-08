// SPDX-License-Identifier: MIT OR Apache-2.0

//! Readers for Assetto Corsa's content files.
//!
//! * [`kn5`]: the `.kn5` model file (textures, materials, the node tree with its meshes).
//!   A track model can be hundreds of megabytes, nearly all of it textures and vertices, so
//!   [`Kn5::open`](kn5::Kn5::open) reads only the structure and remembers where the big
//!   blocks are; a [`Kn5Reader`](kn5::Kn5Reader) fetches one block when it is wanted.
//! * [`track_files`]: which model files make up a track (`models.ini` / `models_<layout>.ini`)
//!   and where its `data` and `ai` folders are.
//!
//! Nothing here is physics: the files that decide what the car feels (`surfaces.ini`, the AI
//! line, the timing gates) are read by `rustyac-physics` with the game's own parsing rules.

pub mod kn5;
pub mod track_files;

pub use kn5::{Kn5, Kn5Reader, Material, MeshInfo, Name, Node, NodeClass, TextureEntry, Vertex};
pub use track_files::{ModelEntry, TrackFiles};
