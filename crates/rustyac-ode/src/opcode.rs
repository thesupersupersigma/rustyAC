// SPDX-License-Identifier: BSD-3-Clause

//! OPCODE as ODE 0.13.1 carries it and `acs.exe` uses it for static triangle meshes: the
//! collision tree of one mesh ([`Model`]) and the ray query against it ([`RayCollider`]).
//!
//! The tree is a complete binary tree with one triangle per leaf, built top-down ("splatter
//! points": the axis on which the triangles' centres vary most, cut at the mean of all their
//! vertices), then stored without its leaves: `n - 1` nodes for `n` triangles, numbered
//! depth-first with the positive child first. Nothing is quantised or re-ordered, so a ray
//! that stops at its first hit visits triangles in exactly this order: the build has to match
//! the game's bit for bit, and does.
//!
//! OPCODE is by Pierre Terdiman; it ships inside ODE under ODE's licence.

/// `INV3`: the game multiplies by this where the source divides by three.
const INV3: f32 = f32::from_bits(0x3eaa_aaab);

/// A child slot of an [`AabbNoLeafNode`] (`mPosData` / `mNegData`): a triangle, or a node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Child {
    /// A leaf: the triangle's number in the index buffer.
    Triangle(u32),
    /// An inner node: its index in [`Model::nodes`].
    Node(u32),
}

/// `Opcode::AABBNoLeafNode` (0x28 bytes): the box of everything below, and the two children.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AabbNoLeafNode {
    /// `mAABB.mCenter`
    pub center: [f32; 3],
    /// `mAABB.mExtents`
    pub extents: [f32; 3],
    /// `mPosData`
    pub pos: Child,
    /// `mNegData`
    pub neg: Child,
}

/// The vertices and 16-bit indices of a mesh (`Opcode::MeshInterface` with the strides the
/// game uses: 12-byte vertices, 6-byte triangles).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshInterface {
    pub vertices: Vec<[f32; 3]>,
    pub indices: Vec<u16>,
    /// `mNbTris`: `indices.len() / 3`; one or two indices left over are ignored.
    pub nb_tris: u32,
}

impl MeshInterface {
    pub fn new(vertices: Vec<[f32; 3]>, indices: Vec<u16>) -> MeshInterface {
        // dxTriMeshData::Build @ 0x14034a780: a signed division, towards zero
        let nb_tris = (indices.len() as i32 / 3) as u32;
        MeshInterface { vertices, indices, nb_tris }
    }

    /// `MeshInterface::IsValid` @ 0x14038ffe0
    pub fn is_valid(&self) -> bool {
        self.nb_tris != 0 && !self.vertices.is_empty()
    }

    /// `MeshInterface::FetchTriangleFromSingles` @ 0x14038ff90: the three corners of a triangle.
    #[inline]
    pub fn triangle(&self, index: u32) -> [&[f32; 3]; 3] {
        let t = index as usize * 3;
        [
            &self.vertices[self.indices[t] as usize],
            &self.vertices[self.indices[t + 1] as usize],
            &self.vertices[self.indices[t + 2] as usize],
        ]
    }
}

/// `Opcode::Model` after `Model::Build` @ 0x1403903b0 with the settings of
/// `dxTriMeshData::Build`: no leaves, not quantised, one triangle per leaf, the "splatter
/// points" split (`mRules` = 0x26, of which only that bit acts).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Model {
    /// `mHybridTree->mNodes`: `n - 1` nodes, the root first. Empty for a mesh of one triangle
    /// (`OPC_SINGLE_NODE`) and for a mesh that could not be built.
    pub nodes: Vec<AabbNoLeafNode>,
    /// `mModelCode & OPC_SINGLE_NODE`: the mesh is one triangle and has no tree.
    pub single_node: bool,
    /// `Model::Build` succeeded (`mIMesh` is set).
    pub built: bool,
    /// `mNbInvalidSplits`: splits that put every triangle on one side and were cut in half.
    pub invalid_splits: u32,
}

/// `AABBTreeOfTrianglesBuilder::GetSplittingValues` @ 0x140398800 for one axis (also
/// `GetSplittingValue(index, axis)` @ 0x140398620): the triangle's centre.
#[inline]
fn centre(mesh: &MeshInterface, prim: u16, axis: usize) -> f32 {
    let [v0, v1, v2] = mesh.triangle(prim as u32);
    ((v0[axis] + v1[axis]) + v2[axis]) * INV3
}

/// `AABBTreeOfTrianglesBuilder::ComputeGlobalBox` @ 0x1403982d0: centre and half sizes of the
/// box around some triangles.
fn compute_global_box(mesh: &MeshInterface, prims: &[u16]) -> ([f32; 3], [f32; 3]) {
    let mut min = [f32::MAX; 3];
    let mut max = [-f32::MAX; 3];
    for &prim in prims {
        let tri = mesh.triangle(prim as u32);
        for v in tri {
            for c in 0..3 {
                // comiss Min, v ; jb skip: an equal value replaces (the sign of a zero), a NaN does not
                if min[c] >= v[c] {
                    min[c] = v[c];
                }
            }
        }
        for v in tri {
            for c in 0..3 {
                // comiss Max, v ; ja skip: equal replaces, and so does a NaN
                if !(max[c] > v[c]) {
                    max[c] = v[c];
                }
            }
        }
    }
    let mut center = [0.0f32; 3];
    let mut extents = [0.0f32; 3];
    for c in 0..3 {
        center[c] = (min[c] + max[c]) * 0.5;
        extents[c] = (max[c] - min[c]) * 0.5;
    }
    (center, extents)
}

/// `AABBTreeNode::Subdivide` @ 0x140396250 and `AABBTreeNode::Split` @ 0x1403961a0 for a node
/// of two or more triangles: re-orders `prims` and returns how many went to the positive
/// child. The second value says the split was invalid and the node was cut in half instead.
fn subdivide(mesh: &MeshInterface, prims: &mut [u16]) -> (usize, bool) {
    let nb = prims.len() as u32;
    // the axis along which the triangles' centres are spread most
    let mut means = [0.0f32; 3];
    for &prim in prims.iter() {
        for c in 0..3 {
            means[c] += centre(mesh, prim, c);
        }
    }
    let inv = 1.0 / nb as f32;
    for mean in means.iter_mut() {
        *mean *= inv;
    }
    let mut vars = [0.0f32; 3];
    for &prim in prims.iter() {
        for c in 0..3 {
            let d = centre(mesh, prim, c) - means[c];
            vars[c] += d * d;
        }
    }
    let inv = 1.0 / (nb - 1) as f32;
    for var in vars.iter_mut() {
        *var *= inv;
    }
    let mut axis = 0;
    if vars[1] > vars[0] {
        axis = 1;
    }
    if vars[2] > vars[axis] {
        axis = 2;
    }

    // `GetSplittingValue(prims, nb, box, axis)` @ 0x1403986f0 (SPLIT_GEOM_CENTER): the mean of
    // every corner of every triangle, summed one coordinate at a time, really divided
    let mut sum = 0.0f32;
    for &prim in prims.iter() {
        let [v0, v1, v2] = mesh.triangle(prim as u32);
        sum += v0[axis];
        sum += v1[axis];
        sum += v2[axis];
    }
    let threshold = sum / nb.wrapping_mul(3) as f32;

    let mut nb_pos = 0usize;
    for i in 0..prims.len() {
        // strictly above goes to the positive side; equal and NaN stay negative
        if centre(mesh, prims[i], axis) > threshold {
            prims.swap(nb_pos, i);
            nb_pos += 1;
        }
    }
    if nb_pos == 0 || nb_pos == prims.len() {
        // every triangle on one side: half and half, in the order they are in
        ((nb >> 1) as usize, true)
    } else {
        (nb_pos, false)
    }
}

impl Model {
    /// `Opcode::Model::Build` @ 0x1403903b0 (`AABBTree::Build` @ 0x140395ff0, then
    /// `AABBNoLeafTree::Build` @ 0x140396c30) in one pass: both walks of the game go depth
    /// first with the positive child first, and a child only touches its own run of triangles.
    pub fn build(mesh: &MeshInterface) -> Model {
        let mut model = Model::default();
        if !mesh.is_valid() {
            return model;
        }
        model.built = true;
        let n = mesh.nb_tris as usize;
        if n == 1 {
            model.single_node = true;
            return model;
        }
        // `mIndices[i] = (u16) i`: a mesh of more than 65,536 triangles wraps, as in the game
        let mut prims: Vec<u16> = (0..n).map(|i| i as u16).collect();
        model.nodes.reserve_exact(n - 1);
        // (first triangle, one past the last, the node and side this run hangs on)
        let mut pending: Vec<(usize, usize, Option<(u32, bool)>)> = vec![(0, n, None)];
        while let Some((lo, hi, parent)) = pending.pop() {
            let id = model.nodes.len() as u32;
            if let Some((parent, negative)) = parent {
                let node = &mut model.nodes[parent as usize];
                if negative {
                    node.neg = Child::Node(id);
                } else {
                    node.pos = Child::Node(id);
                }
            }
            let (center, extents) = compute_global_box(mesh, &prims[lo..hi]);
            let (nb_pos, invalid) = subdivide(mesh, &mut prims[lo..hi]);
            model.invalid_splits += invalid as u32;
            let mid = lo + nb_pos;
            model.nodes.push(AabbNoLeafNode {
                center,
                extents,
                pos: Child::Triangle(prims[lo] as u32),
                neg: Child::Triangle(prims[mid] as u32),
            });
            // the negative run waits until the whole positive subtree is numbered
            if hi - mid > 1 {
                pending.push((mid, hi, Some((id, true))));
            }
            if nb_pos > 1 {
                pending.push((lo, mid, Some((id, false))));
            }
        }
        model
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A strip of `n` unit squares along x, two triangles each.
    fn strip(n: usize) -> MeshInterface {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        for i in 0..=n {
            vertices.push([i as f32, 0.0, 0.0]);
            vertices.push([i as f32, 0.0, 1.0]);
        }
        for i in 0..n as u16 {
            let a = i * 2;
            indices.extend_from_slice(&[a, a + 1, a + 2, a + 1, a + 3, a + 2]);
        }
        MeshInterface::new(vertices, indices)
    }

    fn leaves(model: &Model) -> Vec<u32> {
        fn walk(model: &Model, child: Child, out: &mut Vec<u32>) {
            match child {
                Child::Triangle(t) => out.push(t),
                Child::Node(id) => {
                    let node = model.nodes[id as usize];
                    walk(model, node.pos, out);
                    walk(model, node.neg, out);
                }
            }
        }
        let mut out = Vec::new();
        if !model.nodes.is_empty() {
            walk(model, Child::Node(0), &mut out);
        }
        out
    }

    #[test]
    fn a_tree_has_one_node_less_than_triangles_and_every_triangle_once() {
        for n in [1usize, 2, 3, 7, 64, 257] {
            let mesh = strip(n);
            let model = Model::build(&mesh);
            assert!(model.built);
            assert_eq!(model.nodes.len(), 2 * n - 1);
            let mut found = leaves(&model);
            found.sort_unstable();
            assert_eq!(found, (0..2 * n as u32).collect::<Vec<_>>());
        }
    }

    #[test]
    fn nodes_are_numbered_depth_first_with_the_positive_child_first() {
        let model = Model::build(&strip(16));
        for (id, node) in model.nodes.iter().enumerate() {
            if let Child::Node(pos) = node.pos {
                assert_eq!(pos as usize, id + 1);
            }
            if let (Child::Node(neg), Child::Triangle(_)) = (node.neg, node.pos) {
                assert_eq!(neg as usize, id + 1);
            }
        }
        // the root's box is the whole strip; the positive side is the high-x half
        assert_eq!(model.nodes[0].center, [8.0, 0.0, 0.5]);
        assert_eq!(model.nodes[0].extents, [8.0, 0.0, 0.5]);
        assert!(model.nodes[1].center[0] > 8.0);
    }

    #[test]
    fn one_triangle_has_no_tree_and_an_empty_mesh_is_not_built() {
        let one = Model::build(&MeshInterface::new(vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]], vec![0, 1, 2]));
        assert!(one.built && one.single_node && one.nodes.is_empty());
        let none = Model::build(&MeshInterface::new(vec![[0.0; 3]], vec![0, 0]));
        assert!(!none.built && none.nodes.is_empty());
    }

    #[test]
    fn identical_triangles_are_cut_in_half_in_their_order() {
        // four copies of one triangle: no axis separates them
        let vertices = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
        let model = Model::build(&MeshInterface::new(vertices, vec![0, 1, 2, 0, 1, 2, 0, 1, 2, 0, 1, 2]));
        assert_eq!(model.invalid_splits, 3);
        assert_eq!(leaves(&model), vec![0, 1, 2, 3]);
    }
}
