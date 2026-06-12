//! Socket-aware automatic layout of nodes.
//!
//! A dependency-free, port-aware layered ("Sugiyama") layout: nodes are
//! assigned to layers along the flow direction, ordered within layers to
//! minimise edge crossings, and positioned so that edges run as straight as
//! possible - all computed per *socket* connection point rather than per node
//! centre, following Schulze, Spönemann and von Hanxleden, "Drawing Layered
//! Graphs with Port Constraints" (JVLC 2014).

use crate::edge::{InputIx, OutputIx};
use crate::{Layout, NodeId};
use std::collections::{BTreeMap, HashMap};

mod acyclic;
mod order;
mod place;
mod rank;

/// Per-node input to [`layout`]: the node's size and the layout of its
/// sockets.
pub struct LayoutNode {
    size: egui::Vec2,
    socket_padding: f32,
    inputs: LayoutSockets,
    outputs: LayoutSockets,
}

/// Parameters controlling [`layout`].
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct LayoutParams {
    /// The direction in which edges should flow.
    pub flow: egui::Direction,
    /// The gap between adjacent layers along the flow direction.
    pub layer_gap: f32,
    /// The gap between adjacent nodes within a layer.
    pub node_gap: f32,
    /// The gap between disconnected components of the graph.
    pub component_gap: f32,
}

/// Socket positions along a node's edge, mirroring
/// [`SocketLayout`](crate::SocketLayout).
enum LayoutSockets {
    EvenlySpaced(usize),
    /// Cross-axis offsets relative to the node's top-left corner.
    Explicit(Vec<f32>),
}

/// A connected component of the graph, canonicalised into layout space: the
/// main axis points from sources toward sinks and the cross axis spans each
/// layer.
struct CGraph {
    /// Main-axis extent of each node.
    size_main: Vec<f32>,
    /// Cross-axis extent of each node.
    size_cross: Vec<f32>,
    /// Cross-axis offsets of each node's input sockets from its centre.
    in_anchors: Vec<Vec<f32>>,
    /// Cross-axis offsets of each node's output sockets from its centre.
    out_anchors: Vec<Vec<f32>>,
    edges: Vec<CEdge>,
}

/// An edge between component-local node indices.
struct CEdge {
    src: usize,
    src_socket: usize,
    dst: usize,
    dst_socket: usize,
}

/// The bounding box of a laid-out component in canonical space.
struct Bounds {
    min_main: f32,
    max_main: f32,
    min_cross: f32,
    max_cross: f32,
}

impl LayoutNode {
    /// A node of the given size with no sockets.
    pub fn new(size: impl Into<egui::Vec2>) -> Self {
        Self {
            size: size.into(),
            socket_padding: 0.0,
            inputs: LayoutSockets::EvenlySpaced(0),
            outputs: LayoutSockets::EvenlySpaced(0),
        }
    }

    /// The padding used when deriving evenly spaced socket positions.
    ///
    /// Pass [`socket_padding`](crate::socket_padding) for exact agreement
    /// with the socket positions used when rendering nodes. Defaults to
    /// `0.0`.
    pub fn socket_padding(mut self, padding: f32) -> Self {
        self.socket_padding = padding;
        self
    }

    /// The number of input sockets, evenly spaced along the node's edge.
    pub fn inputs(mut self, count: usize) -> Self {
        self.inputs = LayoutSockets::EvenlySpaced(count);
        self
    }

    /// The number of output sockets, evenly spaced along the node's edge.
    pub fn outputs(mut self, count: usize) -> Self {
        self.outputs = LayoutSockets::EvenlySpaced(count);
        self
    }

    /// Explicit input socket offsets along the cross axis (`y` for horizontal
    /// flows, `x` for vertical), relative to the node's top-left corner.
    ///
    /// Non-finite offsets anchor at the node's cross-axis centre.
    pub fn input_offsets(mut self, offsets: Vec<f32>) -> Self {
        self.inputs = LayoutSockets::Explicit(offsets);
        self
    }

    /// Explicit output socket offsets along the cross axis (`y` for
    /// horizontal flows, `x` for vertical), relative to the node's top-left
    /// corner.
    ///
    /// Non-finite offsets anchor at the node's cross-axis centre.
    pub fn output_offsets(mut self, offsets: Vec<f32>) -> Self {
        self.outputs = LayoutSockets::Explicit(offsets);
        self
    }
}

impl LayoutParams {
    /// Default gap between adjacent layers along the flow direction.
    pub const DEFAULT_LAYER_GAP: f32 = 50.0;
    /// Default gap between adjacent nodes within a layer.
    pub const DEFAULT_NODE_GAP: f32 = 25.0;
    /// Default gap between disconnected components of the graph.
    pub const DEFAULT_COMPONENT_GAP: f32 = 50.0;

    /// Parameters for the given flow direction with default gaps.
    pub fn new(flow: egui::Direction) -> Self {
        Self {
            flow,
            layer_gap: Self::DEFAULT_LAYER_GAP,
            node_gap: Self::DEFAULT_NODE_GAP,
            component_gap: Self::DEFAULT_COMPONENT_GAP,
        }
    }

    /// Set the gap between adjacent layers along the flow direction.
    pub fn layer_gap(mut self, gap: f32) -> Self {
        self.layer_gap = gap;
        self
    }

    /// Set the gap between adjacent nodes within a layer.
    pub fn node_gap(mut self, gap: f32) -> Self {
        self.node_gap = gap;
        self
    }

    /// Set the gap between disconnected components of the graph.
    pub fn component_gap(mut self, gap: f32) -> Self {
        self.component_gap = gap;
        self
    }
}

impl Bounds {
    fn main_extent(&self) -> f32 {
        self.max_main - self.min_main
    }

    fn cross_extent(&self) -> f32 {
        self.max_cross - self.min_cross
    }
}

impl From<egui::Direction> for LayoutParams {
    fn from(flow: egui::Direction) -> Self {
        Self::new(flow)
    }
}

/// Construct a layout for a directed graph from its nodes' sizes and socket
/// layouts and the socket-level connections between them.
///
/// Edges are identified by `(node, output socket)` -> `(node, input socket)`
/// pairs, matching [`Edge::new`](crate::edge::Edge::new). Socket indices
/// without a known offset anchor at the node's cross-axis centre. Edges
/// referencing unknown nodes and self-loops are ignored; when a node id
/// occurs more than once, the last occurrence wins.
///
/// Returns a [`Layout`] with the position of each node's top-left corner,
/// with the bounding box of the laid-out graph centred around the origin.
/// The same input always produces the same layout.
pub fn layout(
    nodes: impl IntoIterator<Item = (NodeId, LayoutNode)>,
    edges: impl IntoIterator<Item = ((NodeId, OutputIx), (NodeId, InputIx))>,
    params: impl Into<LayoutParams>,
) -> Layout {
    let params = params.into();
    let horizontal = matches!(
        params.flow,
        egui::Direction::LeftToRight | egui::Direction::RightToLeft
    );

    // Canonicalise the nodes in a deterministic order.
    let nodes: BTreeMap<NodeId, LayoutNode> = nodes.into_iter().collect();
    let ids: Vec<NodeId> = nodes.keys().copied().collect();
    let index_of: HashMap<NodeId, usize> = ids.iter().enumerate().map(|(i, &id)| (id, i)).collect();
    let mut size_screen = Vec::with_capacity(ids.len());
    let mut size_main = Vec::with_capacity(ids.len());
    let mut size_cross = Vec::with_capacity(ids.len());
    let mut in_anchors = Vec::with_capacity(ids.len());
    let mut out_anchors = Vec::with_capacity(ids.len());
    for node in nodes.values() {
        let size = sanitize_size(node.size);
        let (main, cross) = if horizontal {
            (size.x, size.y)
        } else {
            (size.y, size.x)
        };
        size_screen.push(size);
        size_main.push(main);
        size_cross.push(cross);
        in_anchors.push(resolve_anchors(&node.inputs, cross, node.socket_padding));
        out_anchors.push(resolve_anchors(&node.outputs, cross, node.socket_padding));
    }

    // Sanitise the edges; their order must not affect the result.
    let mut cedges: Vec<CEdge> = edges
        .into_iter()
        .filter_map(|((a, src_socket), (b, dst_socket))| {
            let (Some(&src), Some(&dst)) = (index_of.get(&a), index_of.get(&b)) else {
                return None;
            };
            (src != dst).then_some(CEdge {
                src,
                src_socket,
                dst,
                dst_socket,
            })
        })
        .collect();
    cedges.sort_by_key(|e| (e.src, e.dst, e.src_socket, e.dst_socket));

    // Split into weakly-connected components.
    let mut parent: Vec<usize> = (0..ids.len()).collect();
    for e in &cedges {
        let (ra, rb) = (uf_find(&mut parent, e.src), uf_find(&mut parent, e.dst));
        if ra != rb {
            parent[ra.max(rb)] = ra.min(rb);
        }
    }
    let mut members_by_root: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    let mut local_of = vec![0usize; ids.len()];
    for v in 0..ids.len() {
        let members = members_by_root.entry(uf_find(&mut parent, v)).or_default();
        local_of[v] = members.len();
        members.push(v);
    }
    let mut edges_by_root: BTreeMap<usize, Vec<CEdge>> = BTreeMap::new();
    for e in cedges {
        let root = uf_find(&mut parent, e.src);
        edges_by_root.entry(root).or_default().push(CEdge {
            src: local_of[e.src],
            src_socket: e.src_socket,
            dst: local_of[e.dst],
            dst_socket: e.dst_socket,
        });
    }

    // Lay out each component independently in canonical space.
    let mut placed: Vec<(Vec<usize>, Vec<(f32, f32)>, Bounds)> = members_by_root
        .into_iter()
        .map(|(root, members)| {
            let cg = CGraph {
                size_main: members.iter().map(|&v| size_main[v]).collect(),
                size_cross: members.iter().map(|&v| size_cross[v]).collect(),
                in_anchors: members
                    .iter()
                    .map(|&v| std::mem::take(&mut in_anchors[v]))
                    .collect(),
                out_anchors: members
                    .iter()
                    .map(|&v| std::mem::take(&mut out_anchors[v]))
                    .collect(),
                edges: edges_by_root.remove(&root).unwrap_or_default(),
            };
            let (centers, bounds) = layout_connected(&cg, &params);
            (members, centers, bounds)
        })
        .collect();

    // Stack the components along the cross axis, largest first.
    placed.sort_by(|a, b| {
        b.2.main_extent()
            .total_cmp(&a.2.main_extent())
            .then(b.2.cross_extent().total_cmp(&a.2.cross_extent()))
            .then(a.0[0].cmp(&b.0[0]))
    });
    let mut tls: Vec<(usize, egui::Pos2)> = Vec::with_capacity(ids.len());
    let mut cross_cursor = 0.0;
    for (members, centers, bounds) in &placed {
        let main_shift = -bounds.min_main;
        let cross_shift = cross_cursor - bounds.min_cross;
        for (&global, &(main, cross)) in members.iter().zip(centers) {
            let main = main + main_shift;
            let cross = cross + cross_shift;
            // Map canonical coordinates back to screen space.
            let main = match params.flow {
                egui::Direction::LeftToRight | egui::Direction::TopDown => main,
                egui::Direction::RightToLeft | egui::Direction::BottomUp => -main,
            };
            let center = if horizontal {
                egui::Pos2::new(main, cross)
            } else {
                egui::Pos2::new(cross, main)
            };
            tls.push((global, center - size_screen[global] * 0.5));
        }
        cross_cursor += bounds.cross_extent() + params.component_gap;
    }

    // Centre the overall bounding box around the origin.
    let mut min = egui::Pos2::new(f32::INFINITY, f32::INFINITY);
    let mut max = egui::Pos2::new(f32::NEG_INFINITY, f32::NEG_INFINITY);
    for &(global, tl) in &tls {
        min = min.min(tl);
        max = max.max(tl + size_screen[global]);
    }
    let center_shift = if tls.is_empty() {
        egui::Vec2::ZERO
    } else {
        (min.to_vec2() + max.to_vec2()) * 0.5
    };
    tls.into_iter()
        .map(|(global, tl)| (ids[global], tl - center_shift))
        .collect()
}

/// Construct a layout from node sizes alone.
///
/// A convenience over [`layout`] for graphs without socket information:
/// every edge anchors at its nodes' cross-axis centres.
pub fn layout_from_sizes(
    nodes: impl IntoIterator<Item = (NodeId, egui::Vec2)>,
    edges: impl IntoIterator<Item = (NodeId, NodeId)>,
    flow: egui::Direction,
) -> Layout {
    layout(
        nodes
            .into_iter()
            .map(|(id, size)| (id, LayoutNode::new(size))),
        edges.into_iter().map(|(a, b)| ((a, 0), (b, 0))),
        flow,
    )
}

/// Lay out a single weakly-connected component in canonical space, returning
/// the `(main, cross)` centre of each node and the component bounds.
fn layout_connected(cg: &CGraph, params: &LayoutParams) -> (Vec<(f32, f32)>, Bounds) {
    let n = cg.size_main.len();
    let pairs: Vec<(usize, usize)> = cg.edges.iter().map(|e| (e.src, e.dst)).collect();
    let reversed = acyclic::break_cycles(n, &pairs);
    let oriented: Vec<(usize, usize)> = pairs
        .iter()
        .zip(&reversed)
        .map(|(&(src, dst), &rev)| if rev { (dst, src) } else { (src, dst) })
        .collect();
    let layer = rank::assign_layers(n, &oriented);
    let mut pg = order::build_proper(cg, &reversed, &layer);
    order::minimize_crossings(&mut pg);

    // Dummy vertices occupy no space.
    let num_v = pg.pos.len();
    let mut size_main_all = vec![0.0f32; num_v];
    let mut size_cross_all = vec![0.0f32; num_v];
    size_main_all[..n].copy_from_slice(&cg.size_main);
    size_cross_all[..n].copy_from_slice(&cg.size_cross);
    let cross = place::assign_cross(&pg, &size_cross_all, params.node_gap);
    let main = place::assign_main(&pg, &size_main_all, params.layer_gap);

    let centers: Vec<(f32, f32)> = (0..n).map(|v| (main[v], cross[v])).collect();
    let mut bounds = Bounds {
        min_main: f32::INFINITY,
        max_main: f32::NEG_INFINITY,
        min_cross: f32::INFINITY,
        max_cross: f32::NEG_INFINITY,
    };
    for (v, &(main, cross)) in centers.iter().enumerate() {
        bounds.min_main = bounds.min_main.min(main - cg.size_main[v] * 0.5);
        bounds.max_main = bounds.max_main.max(main + cg.size_main[v] * 0.5);
        bounds.min_cross = bounds.min_cross.min(cross - cg.size_cross[v] * 0.5);
        bounds.max_cross = bounds.max_cross.max(cross + cg.size_cross[v] * 0.5);
    }
    (centers, bounds)
}

/// Cross-axis socket anchor offsets relative to the node's centre.
fn resolve_anchors(sockets: &LayoutSockets, cross_len: f32, socket_padding: f32) -> Vec<f32> {
    let padding = if socket_padding.is_finite() {
        socket_padding
    } else {
        0.0
    };
    let center = cross_len * 0.5;
    match sockets {
        LayoutSockets::EvenlySpaced(count) => {
            crate::socket::layout::evenly_spaced_cross_offsets(*count, cross_len, padding)
                .map(|offset| offset - center)
                .collect()
        }
        LayoutSockets::Explicit(offsets) => offsets
            .iter()
            .map(|&offset| {
                if offset.is_finite() {
                    offset - center
                } else {
                    0.0
                }
            })
            .collect(),
    }
}

fn sanitize_size(size: egui::Vec2) -> egui::Vec2 {
    let clean = |v: f32| if v.is_finite() { v.max(0.0) } else { 0.0 };
    egui::Vec2::new(clean(size.x), clean(size.y))
}

/// The root of `v`, with path compression.
fn uf_find(parent: &mut [usize], v: usize) -> usize {
    let mut root = v;
    while parent[root] != root {
        root = parent[root];
    }
    let mut cur = v;
    while parent[cur] != root {
        let next = parent[cur];
        parent[cur] = root;
        cur = next;
    }
    root
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nid(v: u64) -> NodeId {
        NodeId::from_u64(v)
    }

    /// The centre of the node's rect within `l`.
    fn center(l: &Layout, id: u64, size: egui::Vec2) -> egui::Pos2 {
        l[&nid(id)] + size * 0.5
    }

    fn simple_nodes(count: u64, size: [f32; 2]) -> Vec<(NodeId, LayoutNode)> {
        (0..count)
            .map(|v| (nid(v), LayoutNode::new(size).inputs(1).outputs(1)))
            .collect()
    }

    #[test]
    fn empty_graph() {
        let l = layout(Vec::new(), Vec::new(), egui::Direction::LeftToRight);
        assert!(l.is_empty());
    }

    #[test]
    fn single_node_centred_at_origin() {
        let nodes = vec![(nid(0), LayoutNode::new([100.0, 50.0]))];
        let l = layout(nodes, Vec::new(), egui::Direction::LeftToRight);
        assert_eq!(l[&nid(0)], egui::Pos2::new(-50.0, -25.0));
    }

    #[test]
    fn degenerate_edges_are_total() {
        let nodes = simple_nodes(2, [100.0, 50.0]);
        let edges = vec![
            ((nid(0), 0), (nid(1), 0)),
            ((nid(0), 0), (nid(1), 0)), // multi-edge
            ((nid(0), 0), (nid(0), 0)), // self-loop
            ((nid(7), 0), (nid(1), 0)), // unknown node
            ((nid(0), 9), (nid(1), 9)), // out-of-range sockets
        ];
        let l = layout(nodes, edges, egui::Direction::LeftToRight);
        assert_eq!(l.len(), 2);
        assert!(l.values().all(|p| p.x.is_finite() && p.y.is_finite()));
    }

    #[test]
    fn unknown_node_edges_are_ignored() {
        let with_unknown = layout(
            simple_nodes(2, [100.0, 50.0]),
            vec![
                ((nid(0), 0), (nid(1), 0)),
                ((nid(9), 0), (nid(1), 0)),
                ((nid(0), 0), (nid(9), 0)),
            ],
            egui::Direction::LeftToRight,
        );
        let without = layout(
            simple_nodes(2, [100.0, 50.0]),
            vec![((nid(0), 0), (nid(1), 0))],
            egui::Direction::LeftToRight,
        );
        assert_eq!(with_unknown, without);
    }

    #[test]
    fn deterministic() {
        let nodes = || simple_nodes(5, [100.0, 50.0]);
        let edges = || {
            vec![
                ((nid(0), 0), (nid(1), 0)),
                ((nid(0), 0), (nid(2), 0)),
                ((nid(1), 0), (nid(3), 0)),
                ((nid(2), 0), (nid(3), 0)),
                ((nid(3), 0), (nid(4), 0)),
            ]
        };
        let a = layout(nodes(), edges(), egui::Direction::LeftToRight);
        let b = layout(nodes(), edges(), egui::Direction::LeftToRight);
        assert_eq!(a, b);
        // Nor should the caller's edge order matter.
        let mut rev = edges();
        rev.reverse();
        let c = layout(nodes(), rev, egui::Direction::LeftToRight);
        assert_eq!(a, c);
    }

    #[test]
    fn flow_directions_mirror_and_transpose() {
        let size = egui::Vec2::new(80.0, 80.0);
        let nodes = || simple_nodes(4, [80.0, 80.0]);
        let edges = || {
            vec![
                ((nid(0), 0), (nid(1), 0)),
                ((nid(0), 0), (nid(2), 0)),
                ((nid(1), 0), (nid(3), 0)),
                ((nid(2), 0), (nid(3), 0)),
            ]
        };
        let ltr = layout(nodes(), edges(), egui::Direction::LeftToRight);
        let rtl = layout(nodes(), edges(), egui::Direction::RightToLeft);
        let td = layout(nodes(), edges(), egui::Direction::TopDown);
        for v in 0..4 {
            let c_ltr = center(&ltr, v, size);
            let c_rtl = center(&rtl, v, size);
            assert!((c_rtl.x + c_ltr.x).abs() < 1e-3);
            assert!((c_rtl.y - c_ltr.y).abs() < 1e-3);
            // Square nodes: top-down is the transpose of left-to-right.
            let c_td = center(&td, v, size);
            assert!((c_td.x - c_ltr.y).abs() < 1e-3);
            assert!((c_td.y - c_ltr.x).abs() < 1e-3);
        }
    }

    #[test]
    fn bounding_box_centred_on_origin() {
        let size = egui::Vec2::new(100.0, 50.0);
        let l = layout(
            simple_nodes(4, [100.0, 50.0]),
            vec![
                ((nid(0), 0), (nid(1), 0)),
                ((nid(1), 0), (nid(2), 0)),
                // `3` is a disconnected component.
            ],
            egui::Direction::LeftToRight,
        );
        let mut min = egui::Pos2::new(f32::INFINITY, f32::INFINITY);
        let mut max = egui::Pos2::new(f32::NEG_INFINITY, f32::NEG_INFINITY);
        for &tl in l.values() {
            min = min.min(tl);
            max = max.max(tl + size);
        }
        assert!((min.x + max.x).abs() < 1e-3);
        assert!((min.y + max.y).abs() < 1e-3);
    }

    #[test]
    fn nodes_never_overlap() {
        let size = egui::Vec2::new(100.0, 50.0);
        // Two components: a diamond with a feedback edge, and a lone pair.
        let l = layout(
            simple_nodes(7, [100.0, 50.0]),
            vec![
                ((nid(0), 0), (nid(1), 0)),
                ((nid(0), 0), (nid(2), 0)),
                ((nid(1), 0), (nid(3), 0)),
                ((nid(2), 0), (nid(3), 0)),
                ((nid(3), 0), (nid(0), 0)), // cycle
                ((nid(0), 0), (nid(3), 0)), // long edge
                ((nid(4), 0), (nid(5), 0)),
                // `6` is isolated.
            ],
            egui::Direction::LeftToRight,
        );
        let rects: Vec<egui::Rect> = l
            .values()
            .map(|&tl| egui::Rect::from_min_size(tl, size))
            .collect();
        for (i, a) in rects.iter().enumerate() {
            for b in &rects[i + 1..] {
                assert!(!a.intersects(b.shrink(1.0)), "{a:?} overlaps {b:?}");
            }
        }
    }

    #[test]
    fn straight_chain_has_aligned_sockets() {
        // 1-in/1-out nodes of differing heights: socket anchors (not centres)
        // should align so the edges run straight.
        let sizes = [
            egui::Vec2::new(100.0, 40.0),
            egui::Vec2::new(100.0, 80.0),
            egui::Vec2::new(100.0, 120.0),
        ];
        let padding = 10.0;
        let nodes: Vec<_> = sizes
            .iter()
            .enumerate()
            .map(|(v, &size)| {
                let node = LayoutNode::new(size)
                    .socket_padding(padding)
                    .inputs(1)
                    .outputs(1);
                (nid(v as u64), node)
            })
            .collect();
        let edges = vec![((nid(0), 0), (nid(1), 0)), ((nid(1), 0), (nid(2), 0))];
        let l = layout(nodes, edges, egui::Direction::LeftToRight);
        // A lone socket sits `padding` below the node's top edge.
        let socket_y = |v: u64| l[&nid(v)].y + padding;
        assert!((socket_y(0) - socket_y(1)).abs() < 1e-3);
        assert!((socket_y(1) - socket_y(2)).abs() < 1e-3);
    }

    #[test]
    fn duplicate_ids_last_wins() {
        let nodes = vec![
            (nid(0), LayoutNode::new([10.0, 10.0])),
            (nid(0), LayoutNode::new([100.0, 50.0])),
        ];
        let l = layout(nodes, Vec::new(), egui::Direction::LeftToRight);
        assert_eq!(l[&nid(0)], egui::Pos2::new(-50.0, -25.0));
    }
}
