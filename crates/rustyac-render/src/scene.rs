// SPDX-License-Identifier: GPL-3.0-or-later
// Ported from Assetto Corsa (acs.exe, Kunos Simulazioni).

//! The scene graph and its two traversals: `Node`, `Renderable`, `Mesh`, `NodeBoundingSphere`,
//! `NodeEvent`; `WorldMatrixTraverser::traverse` 0x14021abf0; `Node::render` 0x14020e510,
//! `Mesh::render` 0x1402261a0, `NodeBoundingSphere::render` 0x140218b80;
//! `CameraMeshFilter::isVisible` 0x140219af0; `BoundingFrustum::setMatrix` 0x140229290.
//!
//! The game draws while it walks: depth first, children in the order they were added, every
//! pass over the whole graph. (Its `PvsProcessor`, with a draw-call list and a sort, is called
//! every pass and never given a mesh: nothing of it is ported.)

use rustyac_math::sqrtf;
use rustyac_physics::vecmath::{xm_matrix_multiply, Mat44f, Vec3f};

use crate::graphics::Graphics;
use crate::kgl::{KglCBuffer, KglIndexBuffer, KglVertexBuffer};
use crate::material::{Material, MaterialFilter, MaterialId, PASS_SHADOW, PASS_TRANSPARENT};

pub type NodeId = usize;

/// `sphere`: a centre and a radius.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sphere {
    pub center: Vec3f,
    pub radius: f32,
}

/// `mat44f::getScale` 0x1401f9b80.
pub fn get_scale(m: &Mat44f) -> Vec3f {
    let m = &m.m;
    let sx2 = (m[0][1] * m[0][1] + m[0][0] * m[0][0]) + m[0][2] * m[0][2];
    let sy2 = (m[1][1] * m[1][1] + m[1][0] * m[1][0]) + m[1][2] * m[1][2];
    let sz2 = (m[2][1] * m[2][1] + m[2][0] * m[2][0]) + m[2][2] * m[2][2];
    // `ucomiss; je`: a NaN counts as zero
    let root = |v: f32| if v == 0.0 || v.is_nan() { 0.0 } else { sqrtf(v) };
    Vec3f::new(root(sx2), root(sy2), root(sz2))
}

impl Sphere {
    /// `sphere::transform` 0x140218c00. The radius is left alone as soon as one axis has a
    /// scale of exactly 1.
    pub fn transform(&self, m: &Mat44f) -> Sphere {
        let c = self.center;
        let r = self.radius;
        let m4 = &m.m;
        let x = ((c.y * m4[1][0] + c.x * m4[0][0]) + c.z * m4[2][0]) + m4[3][0];
        let y = ((c.x * m4[0][1] + c.y * m4[1][1]) + c.z * m4[2][1]) + m4[3][1];
        let z = ((c.x * m4[0][2] + c.y * m4[1][2]) + c.z * m4[2][2]) + m4[3][2];
        let s = get_scale(m);
        let is_one = |v: f32| v == 1.0 || v.is_nan();
        let radius = if is_one(s.x) || is_one(s.y) || is_one(s.z) {
            r
        } else {
            let big = if s.x > s.y { s.x } else { s.y };
            if big > s.z {
                (if s.x > s.y { s.x } else { s.y }) * r
            } else {
                s.z * r
            }
        };
        Sphere { center: Vec3f::new(x, y, z), radius }
    }
}

/// `BoundingFrustum` (0xe8 bytes): six planes that point out of the view volume.
#[derive(Clone, Copy, Debug, Default)]
pub struct BoundingFrustum {
    pub mat_view_proj: Mat44f,
    /// near, far, left, right, top, bottom: (a, b, c, d)
    pub planes: [[f32; 4]; 6],
}

impl BoundingFrustum {
    /// `BoundingFrustum::setMatrix` 0x140229290. (The inverse it also stores is used by
    /// `getCorners` only, which the shadow cascades call on a frustum of their own.)
    pub fn set_matrix(&mut self, matrix: &Mat44f) {
        self.mat_view_proj = *matrix;
        let m = &matrix.m;
        let p = &mut self.planes;
        p[2] = [-m[0][3] - m[0][0], -m[1][3] - m[1][0], -m[2][3] - m[2][0], -m[3][3] - m[3][0]];
        p[3] = [m[0][0] - m[0][3], m[1][0] - m[1][3], m[2][0] - m[2][3], m[3][0] - m[3][3]];
        p[4] = [m[0][1] - m[0][3], m[1][1] - m[1][3], m[2][1] - m[2][3], m[3][1] - m[3][3]];
        p[5] = [-m[0][3] - m[0][1], -m[1][3] - m[1][1], -m[2][3] - m[2][1], -m[3][3] - m[3][1]];
        p[0] = [-m[0][2], -m[1][2], -m[2][2], -m[3][2]];
        p[1] = [m[0][2] - m[0][3], m[1][2] - m[1][3], m[2][2] - m[2][3], m[3][2] - m[3][3]];
        for plane in p.iter_mut() {
            let [a, b, c, d] = *plane;
            let l2 = (a * a + b * b) + c * c;
            let zero = |v: f32| v == 0.0 || v.is_nan();
            let mut degenerate = zero(l2);
            if !degenerate {
                let l = sqrtf(l2);
                if zero(l) {
                    degenerate = true;
                } else {
                    let inv = 1.0 / l;
                    *plane = [inv * a, inv * b, inv * c, inv * d];
                }
            }
            if degenerate {
                *plane = [0.0, 1.0, 0.0, 0.0];
            }
        }
    }

    /// `BoundingFrustum::intersect` 0x140229220.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    pub fn intersect(&self, s: &Sphere) -> bool {
        for plane in &self.planes {
            let dist = ((s.center.x * plane[0] + s.center.y * plane[1]) + s.center.z * plane[2]) + plane[3];
            if dist > s.radius {
                return false;
            }
        }
        true
    }
}

/// The members of `Renderable` (0x108 bytes).
#[derive(Clone, Debug)]
pub struct Renderable {
    pub cast_shadows: bool,
    pub is_visible: bool,
    pub is_transparent: bool,
    pub no_cull: bool,
    pub bounding_sphere: Sphere,
    pub layer: i32,
    pub lod_in: f32,
    pub lod_out: f32,
    pub is_static: bool,
}

impl Renderable {
    /// `Renderable::Renderable` 0x14021c260.
    pub fn new() -> Renderable {
        Renderable { cast_shadows: true, is_visible: true, is_transparent: false, no_cull: false, bounding_sphere: Sphere::default(), layer: 0, lod_in: 0.0, lod_out: 0.0, is_static: false }
    }
}

impl Default for Renderable {
    fn default() -> Renderable {
        Renderable::new()
    }
}

/// The members of `Mesh` (0x168 bytes).
pub struct MeshData {
    /// `MeshVertex` records of 44 bytes, exactly the bytes of the file
    pub vertices: Vec<u8>,
    pub indices: Vec<u16>,
    pub material: Option<MaterialId>,
    pub is_renderable: bool,
    pub vb: Option<KglVertexBuffer>,
    pub ib: Option<KglIndexBuffer>,
    pub compiled_indices_count: u32,
    pub compiled_vertices_count: u32,
}

/// `SkinnedBone` (0x68 bytes): the node that moves the bone, and the 64 bytes of the file.
pub struct SkinnedBone {
    pub bone: NodeId,
    pub offset_matrix: Mat44f,
}

/// The members of `SkinnedMesh` (0x188 bytes).
pub struct SkinnedMeshData {
    /// `SkinnedMeshVertex` records of 76 bytes, exactly the bytes of the file
    pub vertices: Vec<u8>,
    pub indices: Vec<u16>,
    pub material: Option<MaterialId>,
    /// the bones found while the file was read, then the ones found after it
    pub bones: Vec<SkinnedBone>,
    pub vb: Option<KglVertexBuffer>,
    pub ib: Option<KglIndexBuffer>,
    /// `bones.len() * 64` bytes at slot 13
    pub bones_buffer: Option<KglCBuffer>,
    pub bones_staging_buffer: Vec<u8>,
    pub compiled_indices_count: u32,
    pub compiled_vertices_count: u32,
}

pub enum NodeKind {
    /// `Node`, `Model`, `CarNodeSorter`
    Node,
    /// `Mesh`
    Mesh(Box<MeshData>),
    /// `SkinnedMesh`
    SkinnedMesh(Box<SkinnedMeshData>),
    /// `NodeBoundingSphere`: its world matrix is copied from the delegate when it is drawn
    BoundingSphere { delegate: Option<NodeId> },
    /// `NodeEvent` (0xf8 bytes): its handlers are called whenever a pass reaches the node
    Event { handlers: Vec<std::rc::Rc<std::cell::RefCell<dyn NodeEventHandler>>> },
}

/// `OnNodeRenderEvent`: the node and what the handlers read of the `RenderContext`.
#[derive(Clone, Copy, Debug)]
pub struct OnNodeRenderEvent {
    pub node: NodeId,
    pub pass_id: i32,
    pub max_layer: i32,
    pub camera: CullCamera,
}

/// A handler of a `NodeEvent`'s `evOnRender`.
pub trait NodeEventHandler {
    fn on_node_render(&mut self, scene: &mut Scene, graphics: &mut Graphics, event: &OnNodeRenderEvent);
}

/// `Node` (0xe0 bytes) and what its subclasses add.
pub struct Node {
    pub name: String,
    pub matrix: Mat44f,
    pub matrix_ws: Mat44f,
    pub children: Vec<NodeId>,
    pub parent: Option<NodeId>,
    pub needs_matrix_ws: bool,
    pub is_active: bool,
    pub renderable: Option<Renderable>,
    pub kind: NodeKind,
}

/// What the mesh filter and the bounding-sphere node read of the camera.
#[derive(Clone, Copy, Debug)]
pub struct CullCamera {
    pub fov: f32,
    pub lod_multiplier: f32,
    /// `Camera::matrix` row 3
    pub position: [f32; 3],
    pub frustum: BoundingFrustum,
    pub is_cube_map_camera: bool,
    pub is_mirror: bool,
}

/// `RenderContext` with its `CameraMeshFilter`.
pub struct RenderContext<'a> {
    pub material_filter: &'a mut MaterialFilter,
    pub pass_id: i32,
    pub max_layer: i32,
    pub camera: CullCamera,
}

/// The graph: nodes by index (what the game holds by pointer) and the materials of every
/// model loaded into it.
#[derive(Default)]
pub struct Scene {
    pub nodes: Vec<Node>,
    pub materials: Vec<Material>,
}

impl Scene {
    pub fn new() -> Scene {
        Scene::default()
    }

    fn push(&mut self, name: &str, kind: NodeKind, renderable: Option<Renderable>, needs_matrix_ws: bool) -> NodeId {
        self.nodes.push(Node { name: name.to_string(), matrix: Mat44f::IDENTITY, matrix_ws: Mat44f::IDENTITY, children: Vec::new(), parent: None, needs_matrix_ws, is_active: true, renderable, kind });
        self.nodes.len() - 1
    }

    /// `Node::Node` 0x14020db10.
    pub fn node(&mut self, name: &str) -> NodeId {
        self.push(name, NodeKind::Node, None, true)
    }

    /// `NodeEvent`.
    pub fn node_event(&mut self, name: &str) -> NodeId {
        self.push(name, NodeKind::Event { handlers: Vec::new() }, None, true)
    }

    /// `Event<OnNodeRenderEvent>::addHandler` on a `NodeEvent`.
    pub fn add_event_handler(&mut self, node: NodeId, handler: std::rc::Rc<std::cell::RefCell<dyn NodeEventHandler>>) {
        if let NodeKind::Event { handlers } = &mut self.nodes[node].kind {
            handlers.push(handler);
        }
    }

    /// `Mesh::Mesh` 0x140225be0: the only kind of node that asks for no world matrix.
    pub fn mesh(&mut self, name: &str, renderable: Renderable, mesh: MeshData) -> NodeId {
        self.push(name, NodeKind::Mesh(Box::new(mesh)), Some(renderable), false)
    }

    /// `SkinnedMesh::SkinnedMesh` 0x14022bda0: its own world matrix stays the identity.
    pub fn skinned_mesh(&mut self, name: &str, renderable: Renderable, mesh: SkinnedMeshData) -> NodeId {
        self.push(name, NodeKind::SkinnedMesh(Box::new(mesh)), Some(renderable), false)
    }

    /// `NodeBoundingSphere::NodeBoundingSphere` 0x140218a30.
    pub fn bounding_sphere(&mut self, name: &str, radius: f32) -> NodeId {
        let mut renderable = Renderable::new();
        renderable.is_transparent = true;
        renderable.bounding_sphere.radius = radius;
        self.push(name, NodeKind::BoundingSphere { delegate: None }, Some(renderable), true)
    }

    /// `Node::addChild` 0x14020dd80.
    pub fn add_child(&mut self, parent: NodeId, child: NodeId) {
        if let Some(old) = self.nodes[child].parent {
            self.remove_child(old, child);
        }
        self.nodes[parent].children.push(child);
        self.nodes[child].parent = Some(parent);
    }

    /// `Node::removeChild` 0x14020e4a0.
    pub fn remove_child(&mut self, parent: NodeId, child: NodeId) {
        if let Some(at) = self.nodes[parent].children.iter().position(|c| *c == child) {
            self.nodes[parent].children.remove(at);
        }
        self.nodes[child].parent = None;
    }

    /// `Node::findChildByName` 0x14020de40: the children first to last, each with its own
    /// sub-tree when `recursive`.
    pub fn find_child_by_name(&self, node: NodeId, name: &str, recursive: bool) -> Option<NodeId> {
        for &child in &self.nodes[node].children {
            if self.nodes[child].name == name {
                return Some(child);
            }
            if recursive {
                if let Some(found) = self.find_child_by_name(child, name, true) {
                    return Some(found);
                }
            }
        }
        None
    }

    /// `Node::getWorldMatrix` 0x14020e190: from the local matrices up the tree, as they are
    /// now (not the world matrices of the last traversal).
    pub fn get_world_matrix(&self, node: NodeId) -> Mat44f {
        let n = &self.nodes[node];
        match n.parent {
            Some(parent) => xm_matrix_multiply(&n.matrix, &self.get_world_matrix(parent)),
            None => n.matrix,
        }
    }

    /// `Node::findChildrenByName` 0x14020df50: the node itself and everything below it, each
    /// node before its children.
    pub fn find_children_by_name(&self, node: NodeId, name: &str, out: &mut Vec<NodeId>) {
        if self.nodes[node].name == name {
            out.push(node);
        }
        for &child in &self.nodes[node].children {
            self.find_children_by_name(child, name, out);
        }
    }

    /// `Node::findChildrenByPrefix` 0x14020e050: the same walk, for names that start with
    /// `prefix`.
    pub fn find_children_by_prefix(&self, node: NodeId, prefix: &str, out: &mut Vec<NodeId>) {
        if self.nodes[node].name.starts_with(prefix) {
            out.push(node);
        }
        for &child in &self.nodes[node].children {
            self.find_children_by_prefix(child, prefix, out);
        }
    }

    /// `Node::localToWorld` 0x14020e230: a point through the node's world matrix (from the
    /// local matrices, as they are now).
    pub fn local_to_world(&self, node: NodeId, v: &Vec3f) -> Vec3f {
        let w = self.get_world_matrix(node);
        let m = &w.m;
        Vec3f::new(
            (((v.y * m[1][0]) + (v.x * m[0][0])) + (v.z * m[2][0])) + m[3][0],
            (((v.x * m[0][1]) + (v.y * m[1][1])) + (v.z * m[2][1])) + m[3][1],
            (((v.x * m[0][2]) + (v.y * m[1][2])) + (v.z * m[2][2])) + m[3][2],
        )
    }

    /// `NodeBoundingSphere::applyNoCull` 0x140218b50: marks what is below `start` right now.
    pub fn apply_no_cull(&mut self, start: NodeId) {
        let children = self.nodes[start].children.clone();
        for child in children {
            if let Some(renderable) = &mut self.nodes[child].renderable {
                renderable.no_cull = true;
            }
            self.apply_no_cull(child);
        }
    }

    /// `TrackAvatar::processPhysicsNode` 0x1401cc5e0, the part the picture sees: every node of
    /// a track whose name starts with `AC_` (spawn points, timing gates, loose objects …) is
    /// switched off, and with it everything below it.
    pub fn hide_helpers(&mut self, node: NodeId) {
        if self.nodes[node].name.starts_with("AC_") {
            self.nodes[node].is_active = false;
        }
        let children = self.nodes[node].children.clone();
        for child in children {
            self.hide_helpers(child);
        }
    }

    /// `WorldMatrixTraverser::traverse` 0x14021abf0.
    pub fn traverse(&mut self, n: NodeId) {
        let node = &self.nodes[n];
        if !node.is_active {
            return;
        }
        if !node.needs_matrix_ws && node.children.is_empty() {
            return;
        }
        let world = match node.parent {
            Some(parent) => xm_matrix_multiply(&node.matrix, &self.nodes[parent].matrix_ws),
            None => node.matrix,
        };
        self.nodes[n].matrix_ws = world;
        let count = self.nodes[n].children.len();
        for i in 0..count {
            let child = self.nodes[n].children[i];
            self.traverse(child);
        }
    }

    /// `GraphicsManager::compile` 0x1402026e0: `Node::compile` 0x14020ddd0 down the tree,
    /// `Mesh::compile` 0x140225f00 on every mesh.
    pub fn compile(&mut self, graphics: &Graphics, n: NodeId) {
        let name = self.nodes[n].name.clone();
        let node = &mut self.nodes[n];
        if let NodeKind::Mesh(mesh) = &mut node.kind {
            let renderable = node.renderable.as_mut().expect("a mesh is a renderable");
            if mesh.ib.is_some() || mesh.vb.is_some() {
                println!("WARNING: Mesh {name} is already compiled, recompiling");
                renderable.bounding_sphere.radius = 0.0;
            }
            if mesh.vertices.is_empty() {
                println!("WARNING: ZERO VERTICES IN MESH {name}");
            }
            if mesh.indices.is_empty() || !mesh.is_renderable {
                return;
            }
            if mesh.material.is_none() {
                println!("WARNING: MESH {name} HAS NULL MATERIAL");
            }
            let count = mesh.vertices.len() / 44;
            if count > 0xfffa {
                println!("WARNING: MESH {name} HAS {count} vertices");
            }
            mesh.vb = Some(graphics.kgl.create_vertex_buffer(&mesh.vertices, count * 44, 44, false));
            mesh.ib = Some(graphics.kgl.create_index_buffer(&mesh.indices));
            if renderable.bounding_sphere.radius == 0.0 || renderable.bounding_sphere.radius.is_nan() {
                renderable.bounding_sphere = update_bounding_sphere(&mesh.vertices, 44);
            }
            mesh.compiled_indices_count = mesh.indices.len() as u32;
            mesh.compiled_vertices_count = count as u32;
            return;
        }
        if let NodeKind::SkinnedMesh(mesh) = &mut node.kind {
            // SkinnedMesh::compile 0x14022c910
            let renderable = node.renderable.as_mut().expect("a skinned mesh is a renderable");
            if mesh.ib.is_some() || mesh.vb.is_some() {
                println!("WARNING: Skinned Mesh {name} is already compiled");
            }
            if mesh.vertices.is_empty() {
                println!("WARNING ZERO VERTICES IN SKINNED MESH {name}");
            }
            if mesh.indices.is_empty() {
                return;
            }
            if mesh.material.is_none() {
                println!("WARNING: SKINNED MESH {name} has NULL material");
            }
            let count = mesh.vertices.len() / 76;
            if count > 0xfffa {
                println!("WARNING,SKINNED MESH {name} HAS {count} vertices");
            }
            mesh.vb = Some(graphics.kgl.create_vertex_buffer(&mesh.vertices, count * 76, 76, false));
            mesh.ib = Some(graphics.kgl.create_index_buffer(&mesh.indices));
            renderable.bounding_sphere = update_bounding_sphere(&mesh.vertices, 76);
            // SkinnedMesh::initBonesBuffer 0x14022cb30
            let size = mesh.bones.len() * 64;
            mesh.bones_buffer = Some(graphics.kgl.create_cbuffer(size as i32));
            mesh.bones_staging_buffer = vec![0u8; size];
            mesh.compiled_indices_count = mesh.indices.len() as u32;
            mesh.compiled_vertices_count = count as u32;
            return;
        }
        let count = node.children.len();
        for i in 0..count {
            let child = self.nodes[n].children[i];
            self.compile(graphics, child);
        }
    }

    /// The virtual `render` (vtable slot +0x18) of whatever `n` is.
    pub fn render(&mut self, graphics: &mut Graphics, n: NodeId, rc: &mut RenderContext) {
        match &self.nodes[n].kind {
            NodeKind::Mesh(_) => self.render_mesh(graphics, n, rc),
            NodeKind::SkinnedMesh(_) => self.render_skinned_mesh(graphics, n, rc),
            NodeKind::BoundingSphere { delegate } => {
                // NodeBoundingSphere::render 0x140218b80
                if let Some(delegate) = *delegate {
                    self.nodes[n].matrix_ws = self.nodes[delegate].matrix_ws;
                }
                let node = &self.nodes[n];
                let sphere = node.renderable.as_ref().map(|r| r.bounding_sphere).unwrap_or_default().transform(&node.matrix_ws);
                if rc.camera.frustum.intersect(&sphere) {
                    self.render_node(graphics, n, rc);
                }
            }
            NodeKind::Event { handlers } => {
                // NodeEvent::render 0x14021e5c0: the handlers in the order they were added,
                // then the node's children
                if self.nodes[n].is_active {
                    let handlers = handlers.clone();
                    let event = OnNodeRenderEvent { node: n, pass_id: rc.pass_id, max_layer: rc.max_layer, camera: rc.camera };
                    for handler in handlers {
                        handler.borrow_mut().on_node_render(self, graphics, &event);
                    }
                }
                self.render_node(graphics, n, rc)
            }
            NodeKind::Node => self.render_node(graphics, n, rc),
        }
    }

    /// `Node::render` 0x14020e510.
    fn render_node(&mut self, graphics: &mut Graphics, n: NodeId, rc: &mut RenderContext) {
        graphics.set_world_matrix(&self.nodes[n].matrix_ws);
        let count = self.nodes[n].children.len();
        for i in 0..count {
            let child = self.nodes[n].children[i];
            if self.nodes[child].is_active {
                self.render(graphics, child, rc);
            }
        }
    }

    /// `Mesh::render` 0x1402261a0.
    fn render_mesh(&mut self, graphics: &mut Graphics, n: NodeId, rc: &mut RenderContext) {
        let node = &self.nodes[n];
        let NodeKind::Mesh(mesh) = &node.kind else {
            return;
        };
        if mesh.ib.is_none() || mesh.vb.is_none() || !mesh.is_renderable {
            return;
        }
        if let Some(parent) = node.parent {
            let renderable = node.renderable.as_ref().expect("a mesh is a renderable");
            if !is_visible(rc, renderable, &self.nodes[parent].matrix_ws) {
                return;
            }
        }
        if let Some(material) = mesh.material {
            rc.material_filter.apply(material, &mut self.materials[material.0 as usize], graphics, rc.pass_id);
        }
        let NodeKind::Mesh(mesh) = &self.nodes[n].kind else {
            return;
        };
        graphics.set_vb(mesh.vb.as_ref().expect("checked above"));
        graphics.set_ib(mesh.ib.as_ref().expect("checked above"));
        graphics.commit_shader_changes();
        graphics.draw_primitive(mesh.compiled_indices_count as i32, 0, 0);
    }

    /// `SkinnedMesh::render` 0x14022cdc0.
    fn render_skinned_mesh(&mut self, graphics: &mut Graphics, n: NodeId, rc: &mut RenderContext) {
        let node = &self.nodes[n];
        let NodeKind::SkinnedMesh(mesh) = &node.kind else {
            return;
        };
        if mesh.compiled_indices_count < 3 {
            return;
        }
        // the game reads the parent without a test; a skinned mesh always hangs under a node
        let parent_world = node.parent.map(|p| self.nodes[p].matrix_ws).unwrap_or(Mat44f::IDENTITY);
        let renderable = node.renderable.as_ref().expect("a skinned mesh is a renderable");
        if !is_visible(rc, renderable, &parent_world) {
            return;
        }
        if let Some(material) = mesh.material {
            rc.material_filter.apply(material, &mut self.materials[material.0 as usize], graphics, rc.pass_id);
        }
        // SkinnedMesh::updateBonesBuffer 0x14022ce90: offsetMatrix * the bone node's world
        // matrix, rows as they are (not transposed)
        let NodeKind::SkinnedMesh(mesh) = &self.nodes[n].kind else {
            return;
        };
        let mut staging = Vec::with_capacity(mesh.bones.len() * 64);
        for bone in &mesh.bones {
            let s = xm_matrix_multiply(&bone.offset_matrix, &self.nodes[bone.bone].matrix_ws);
            for row in &s.m {
                for v in row {
                    staging.extend(v.to_le_bytes());
                }
            }
        }
        let NodeKind::SkinnedMesh(mesh) = &mut self.nodes[n].kind else {
            return;
        };
        mesh.bones_staging_buffer = staging;
        graphics.set_vb(mesh.vb.as_ref().expect("compiled"));
        graphics.set_ib(mesh.ib.as_ref().expect("compiled"));
        graphics.commit_shader_changes();
        if let Some(buffer) = &mesh.bones_buffer {
            graphics.kgl.cbuffer_map(buffer, &mesh.bones_staging_buffer);
            graphics.kgl.cbuffer_bind(buffer, 13);
        }
        graphics.draw_primitive(mesh.compiled_indices_count as i32, 0, 0);
        // Node::render: the mesh's own world matrix (the identity), then its children
        self.render_node(graphics, n, rc);
    }
}

/// `Mesh::updateBoundingSphere` 0x140226280: the average of the vertices and the farthest one.
fn update_bounding_sphere(vertices: &[u8], stride: usize) -> Sphere {
    let position = |v: &[u8]| {
        let f = |at: usize| f32::from_le_bytes([v[at], v[at + 1], v[at + 2], v[at + 3]]);
        (f(0), f(4), f(8))
    };
    let (mut cx, mut cy, mut cz) = (0.0f32, 0.0f32, 0.0f32);
    let count = vertices.len() / stride;
    for v in vertices.chunks_exact(stride) {
        let (x, y, z) = position(v);
        cx += x;
        cy += y;
        cz += z;
    }
    let inv = 1.0f32 / (count as i64 as f32);
    let (cx, cy, cz) = (inv * cx, inv * cy, inv * cz);
    let mut radius = 0.0f32;
    let mut best = 0.0f32;
    for v in vertices.chunks_exact(stride) {
        let (x, y, z) = position(v);
        let (dx, dy, dz) = (x - cx, y - cy, z - cz);
        let d2 = (dy * dy + dx * dx) + dz * dz;
        let d = if d2 == 0.0 || d2.is_nan() { 0.0 } else { sqrtf(d2) };
        if d > best {
            radius = d;
            best = d;
        }
    }
    Sphere { center: Vec3f::new(cx, cy, cz), radius }
}

/// `CameraMeshFilter::isVisible` 0x140219af0. `world` is the world matrix of the mesh's parent.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub fn is_visible(rc: &RenderContext, m: &Renderable, world: &Mat44f) -> bool {
    if m.layer > rc.max_layer {
        return false;
    }
    let pass = rc.pass_id;
    if pass == PASS_SHADOW && !m.cast_shadows {
        return false;
    }
    if m.is_transparent {
        if pass != PASS_TRANSPARENT && pass != PASS_SHADOW {
            return false;
        }
    } else if pass == PASS_TRANSPARENT {
        return false;
    }
    if pass != PASS_SHADOW && !m.is_visible {
        return false;
    }
    if m.no_cull {
        return true;
    }
    let cam = &rc.camera;
    let x = cam.fov * f32::from_bits(0x3c4c_cccd);
    let f = if x > 1.0 {
        1.0
    } else if x >= 0.0 {
        x
    } else {
        0.0
    };
    let k = f * cam.lod_multiplier;
    let (s, cmp_radius) = if !m.is_static { (m.bounding_sphere.transform(world), m.bounding_sphere.radius) } else { (m.bounding_sphere, m.bounding_sphere.radius) };
    let (lod_in, lod_out) = (m.lod_in, m.lod_out);
    // `ucomiss; jne`: a NaN counts as equal to zero
    let is_zero = |v: f32| v == 0.0 || v.is_nan();
    if !(is_zero(lod_in) && is_zero(lod_out)) {
        let dx = cam.position[0] - s.center.x;
        let dy = cam.position[1] - s.center.y;
        let dz = cam.position[2] - s.center.z;
        let mut d2 = (dy * dy + dx * dx) + dz * dz;
        d2 = (d2 * k) * k;
        let out = if lod_out > cmp_radius { lod_out } else { cmp_radius };
        // `comiss; jb`: taken when below or unordered
        if !(d2 >= lod_in * lod_in) {
            return false;
        }
        if d2 > out * out {
            return false;
        }
    }
    cam.frustum.intersect(&s)
}
