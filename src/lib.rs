use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::Hash;
use std::sync::{Arc, Mutex};

#[cfg(feature = "layout")]
pub use layout::layout;
pub use node::{FramedResponse, NodeCtx, NodeId, NodeInteraction};
pub use socket::layout::{grid::SocketGrid, SocketLayout};
pub use socket::{SocketKind, SocketResponses};

pub mod bezier;
pub mod edge;
#[cfg(feature = "layout")]
pub mod layout;
pub mod node;
pub mod socket;

/// The main interface for the `Graph` widget.
pub struct Graph {
    background: bool,
    dot_grid: bool,
    zoom_range: egui::Rangef,
    max_inner_size: Option<egui::Vec2>,
    center_view: bool,
    id: egui::Id,
    /// If set, overwrite the graph's selected nodes at the start of the frame.
    selected_nodes: Option<HashSet<NodeId>>,
    /// When `true`, prevents structural changes while preserving navigation and
    /// selection.
    ///
    /// Unlike `Ui::set_enabled(false)` which disables all interaction
    /// (including panning, zooming, and selection), `immutable` only prevents
    /// structural changes - node positions, edges, and node content remain
    /// view-only while navigation and selection continue to work.
    immutable: bool,
}

/// State related to the graph UI.
#[derive(Clone, Default)]
pub struct GraphTempMemory {
    /// The most recently recorded size of each node.
    ///
    /// Primarily used to check for node selection, as we don't know the size of the node until the
    /// contents have been instantiated.
    node_sizes: NodeSizes,
    /// The currently selected nodes and edges.
    selection: Selection,
    /// Whether or not the primary button was pressed on the graph area and is still down.
    ///
    /// Used for tracking selection and dragging.
    pressed: Option<Pressed>,
    /// Collect information about the layout of each node's sockets during node instantiation.
    ///
    /// This is used to provide the position and normal of each socket when instantiating edges.
    sockets: HashMap<NodeId, NodeSockets>,
    /// The socket that is currently closest to the mouse.
    ///
    /// Always `Some` while the pointer is over the graph area, `None` otherwise.
    closest_socket: Option<socket::Socket>,
}

type NodeSizes = HashMap<NodeId, egui::Vec2>;

#[derive(Clone, Default)]
struct Selection {
    /// The set of currently selected nodes.
    nodes: HashSet<NodeId>,
    /// Whether the selection was modified this frame.
    changed: bool,
}

/// State related to the last press of the primary pointer button over the graph.
#[derive(Clone, Debug)]
struct Pressed {
    /// Whether or not the pointer is currently over one of the selected nodes.
    ///
    /// This is used to assist with determining whether or not nodes should deselect. E.g. if
    /// multiple nodes are selected and a non-selected node is pressed, then we should deselect the
    /// originally selected nodes. However, if a selected node is pressed, then the selection
    /// should stay the same and a drag will begin.
    over_selection_at_origin: bool,
    /// The origin of the pointer over the graph at the begining of the press.
    origin_pos: egui::Pos2,
    /// The current position over the graph.
    current_pos: egui::Pos2,
    /// The action performed by this press.
    action: PressAction,
}

#[derive(Clone, Debug)]
enum PressAction {
    /// A node was pressed and a drag is taking place.
    DragNodes {
        /// The node that was pressed to initiate the drag.
        ///
        /// We don't know exactly which until the node itself emits the pressed event, so this
        /// remains `None` until we know.
        node: Option<PressedNode>,
    },
    /// The graph was pressed and we are performing a selection.
    Select,
    /// A node's socket was pressed in order to start creating a connection.
    Socket(socket::Socket),
}

#[derive(Clone, Debug)]
struct PressedNode {
    /// Unique Id of the node.
    id: NodeId,
    /// The position of the node over the graph at the origin of the press.
    position_at_origin: egui::Pos2,
}

/// Configuration for the graph.
// TODO: Consider storing this in graph widget "memory"?
// The thing is, it might be nice to let the user modify these externally.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct View {
    /// The visible area of the graph's [`Scene`][egui::containers::Scene].
    pub scene_rect: egui::Rect,
    #[cfg_attr(feature = "serde", serde(serialize_with = "serialize_sorted_layout"))]
    pub layout: Layout,
}

#[cfg(feature = "serde")]
fn serialize_sorted_layout<S: serde::Serializer>(layout: &Layout, s: S) -> Result<S::Ok, S::Error> {
    use serde::Serialize;
    let sorted: BTreeMap<_, _> = layout.iter().collect();
    sorted.serialize(s)
}

/// The location of the top-left of each node relative to the centre of the graph area.
pub type Layout = HashMap<NodeId, egui::Pos2>;

/// The context returned by the `Graph` widget. Allows for setting nodes and edges.
pub struct Show<'a> {
    /// Useful for accessing the `GraphTempMemory`.
    graph_id: egui::Id,
    /// The full area covered by the `Graph` within the UI.
    graph_rect: egui::Rect,
    /// If a selection is being performed with the pointer, this is the covered area.
    selection_rect: Option<egui::Rect>,
    /// Whether or not the primary mouse button was just released to perform the selection.
    select: bool,
    /// The closest socket within pressable range of the pointer.
    closest_socket: Option<socket::Socket>,
    /// Whether or not the primary mouse button was just released to end edge creation.
    socket_press_released: Option<socket::Socket>,
    /// Track all nodes that were visited this update.
    ///
    /// We will use this to remove old node state on `drop`.
    visited: &'a mut HashSet<NodeId>,
    layout: &'a mut Layout,
    /// Whether the graph is in immutable (view-only) mode.
    immutable: bool,
}

/// Information about the inputs and outputs for a particular node.
#[derive(Clone)]
pub struct NodeSockets {
    flow: egui::Direction,
    inputs: BTreeMap<usize, egui::Pos2>,
    outputs: BTreeMap<usize, egui::Pos2>,
}

/// A context to assist with the instantiation of node widgets.
pub struct NodesCtx<'a> {
    pub graph_id: egui::Id,
    graph_rect: egui::Rect,
    selection_rect: Option<egui::Rect>,
    select: bool,
    socket_press_released: Option<socket::Socket>,
    visited: &'a mut HashSet<NodeId>,
    layout: &'a mut Layout,
    /// Whether the graph is in immutable (view-only) mode.
    pub immutable: bool,
}

/// A context to assist with the instantiation of edge widgets.
pub struct EdgesCtx {
    graph_id: egui::Id,
    graph_rect: egui::Rect,
    selection_rect: Option<egui::Rect>,
    closest_socket: Option<socket::Socket>,
    /// Whether the graph is in immutable (view-only) mode.
    pub immutable: bool,
}

/// The set of detected graph interaction for a single graph widget update prior
/// to node interaction.
struct GraphInteraction {
    pressed: Option<Pressed>,
    socket_press_released: Option<socket::Socket>,
    select: bool,
    selection_rect: Option<egui::Rect>,
    drag_nodes_delta: egui::Vec2,
}

/// The response returned by [`Graph::show`].
pub struct GraphResponse<R> {
    /// The user's return value from the content closure.
    pub inner: R,
    /// The egui [`Response`][egui::Response] for the graph's scene area.
    pub response: egui::Response,
    /// The set of selected nodes, present only when the selection changed this frame.
    pub selection_changed: Option<HashSet<NodeId>>,
}

impl Graph {
    /// The default zoom range.
    ///
    /// Allows zooming out 4x, but does not allow zooming in past the
    /// pixel-perfect default level.
    pub const DEFAULT_ZOOM_RANGE: egui::Rangef = egui::Rangef {
        min: 0.25,
        max: 1.0,
    };
    pub const DEFAULT_CENTER_VIEW: bool = false;

    /// Begin building the new graph widget.
    pub fn new(id_src: impl Hash) -> Self {
        Self::from_id(id(id_src))
    }

    /// The same as [`Graph::new`], but allows providing an `egui::Id` directly.
    pub fn from_id(id: egui::Id) -> Self {
        Self {
            background: true,
            dot_grid: true,
            zoom_range: Self::DEFAULT_ZOOM_RANGE,
            max_inner_size: None,
            center_view: Self::DEFAULT_CENTER_VIEW,
            id,
            selected_nodes: None,
            immutable: false,
        }
    }

    /// Whether or not to fill the background. Default is `true`.
    pub fn background(mut self, show: bool) -> Self {
        self.background = show;
        self
    }

    /// Whether or not to show the dot grid. Default is `true`.
    pub fn dot_grid(mut self, show: bool) -> Self {
        self.dot_grid = show;
        self
    }

    /// Set the allowed zoom range.
    ///
    /// A zoom < 1.0 zooms out, while a zoom > 1.0 zooms in.
    ///
    /// Default: [Graph::DEFAULT_ZOOM_RANGE].
    pub fn zoom_range(mut self, zoom_range: impl Into<egui::Rangef>) -> Self {
        self.zoom_range = zoom_range.into();
        self
    }

    /// Set the maximum size of the scene's inner [`Ui`] that will be created.
    #[inline]
    pub fn max_inner_size(mut self, max_inner_size: impl Into<egui::Vec2>) -> Self {
        self.max_inner_size = Some(max_inner_size.into());
        self
    }

    /// Whether or not to center the view around the content of the graph.
    ///
    /// Default: [Self::DEFAULT_CENTER_VIEW].
    pub fn center_view(mut self, center_view: bool) -> Self {
        self.center_view = center_view;
        self
    }

    /// Set the selected nodes for this frame.
    ///
    /// This overwrites the current selection in the graph's temporary memory
    /// at the start of the next `show` call.
    pub fn selected_nodes(mut self, nodes: HashSet<NodeId>) -> Self {
        self.selected_nodes = Some(nodes);
        self
    }

    /// Set immutable (view-only) mode.
    ///
    /// When `true`, prevents structural changes while preserving navigation
    /// and selection. Node dragging, edge creation/deletion, node deletion,
    /// and node content widgets are all disabled.
    ///
    /// Default: `false`.
    pub fn immutable(mut self, immutable: bool) -> Self {
        self.immutable = immutable;
        self
    }

    /// Begin showing the Graph.
    ///
    /// Returns a [`GraphResponse`] containing the user's return value,
    /// the scene [`egui::Response`], and the current set of selected nodes.
    pub fn show<R>(
        mut self,
        view: &mut View,
        ui: &mut egui::Ui,
        content: impl FnOnce(&mut egui::Ui, Show) -> R,
    ) -> GraphResponse<R> {
        // The full area to be occuppied by the graph.
        let graph_rect = ui.available_rect_before_wrap();

        let View {
            ref mut scene_rect,
            ref mut layout,
        } = *view;

        // Create the Scene.
        let mut scene = egui::containers::Scene::new()
            .zoom_range(self.zoom_range)
            .drag_pan_buttons(egui::containers::DragPanButtons::MIDDLE);
        if let Some(max_inner_size) = self.max_inner_size {
            scene = scene.max_inner_size(max_inner_size);
        }

        // Track the bounding area of all widgets in the scene.
        let mut bounding_rect = None;

        let scene_response = scene.show(ui, scene_rect, |ui| {
            // Draw the selection rectangle if there is one.
            let mut selection_rect = None;
            let mut select = false;
            let mut closest_socket = None;
            let mut socket_press_released = None;

            // Check for interactions with the scene area.
            let scene_response = ui.response();
            let ptr_on_graph = scene_response.hovered();

            // Check for selection rectangle and node dragging.
            let gmem_arc = memory(ui, self.id);
            let mut gmem = gmem_arc.lock().expect("failed to lock graph temp memory");

            // Apply externally-provided selection if set.
            if let Some(nodes) = self.selected_nodes.take() {
                gmem.selection.nodes = nodes;
            }

            // Reset the selection dirty flag for this frame.
            gmem.selection.changed = false;

            // FIXME: Here we grab the global pointer and transform its position
            // to the graph scene space in order to check for initialising node
            // drag events. However, doing this means we run the risk of
            // incorrectly responding to events that should be captured by
            // widgets floating above (like a window floating above the graph).
            // We should change this to get the pointer only if it is hovered or
            // interacting with the scene or any of its child nodes somehow.
            let pointer = ui.input(|i| i.pointer.clone());
            if let Some(ptr_global) = pointer.interact_pos().or(pointer.hover_pos()) {
                let ptr_graph = ui
                    .ctx()
                    .layer_transform_from_global(ui.layer_id())
                    .unwrap_or_default()
                    .mul_pos(ptr_global);

                // Check for the closest socket.
                closest_socket = ui.response().hover_pos().and_then(|pos| {
                    find_closest_socket(pos, layout, &gmem, ui).map(|(socket, _dist_sqrd)| socket)
                });

                // When immutable, suppress socket presses (map to Select).
                let closest_socket_for_interaction =
                    if self.immutable { None } else { closest_socket };

                // Check for graph interactions.
                let interaction = graph_interaction(
                    layout,
                    &pointer,
                    closest_socket_for_interaction,
                    ptr_on_graph,
                    ptr_graph,
                    gmem.pressed.as_ref(),
                );

                // Apply drag delta to all selected nodes (skip when immutable).
                if !self.immutable && interaction.drag_nodes_delta != egui::Vec2::ZERO {
                    if let Some(pressed) = gmem.pressed.as_ref() {
                        if let PressAction::DragNodes { .. } = pressed.action {
                            for &n_id in &gmem.selection.nodes {
                                if let Some(pos) = layout.get_mut(&n_id) {
                                    *pos += interaction.drag_nodes_delta;
                                }
                            }
                        }
                    }
                }

                gmem.pressed = interaction.pressed;
                gmem.closest_socket = closest_socket;
                selection_rect = interaction.selection_rect;
                select = interaction.select;
                socket_press_released = interaction.socket_press_released;
            }

            // Paint the background rect.
            let visible_rect = ui.clip_rect();
            if self.background {
                paint_background(visible_rect, ui);
            }

            // Paint some subtle dots to check camera movement.
            if self.dot_grid {
                paint_dot_grid(visible_rect, ui);
            }

            // Draw the selection area if there is one.
            // TODO: Do this when `Show` is `drop`ped or finalised.
            if let Some(sel_rect) = selection_rect {
                paint_selection_area(sel_rect, ui);
            }

            let mut visited = HashSet::default();

            let show = Show {
                graph_id: self.id,
                graph_rect,
                selection_rect,
                select,
                closest_socket,
                socket_press_released,
                visited: &mut visited,
                layout,
                immutable: self.immutable,
            };

            // Drop the lock before running the content.
            std::mem::drop(gmem);

            let output = content(ui, show);

            prune_unused_nodes(self.id, &visited, ui);
            bounding_rect = Some(ui.min_rect());

            // Snapshot selection only if it changed this frame.
            let gmem_arc = memory(ui, self.id);
            let gmem = gmem_arc.lock().expect("failed to lock graph temp memory");
            let selection_changed = if gmem.selection.changed {
                Some(gmem.selection.nodes.clone())
            } else {
                None
            };

            (output, selection_changed)
        });

        if self.center_view {
            if let Some(rect) = bounding_rect {
                view.scene_rect = rect.expand(rect.width() * 0.1);
            }
        }

        let (inner, selection_changed) = scene_response.inner;
        GraphResponse {
            inner,
            response: scene_response.response,
            selection_changed,
        }
    }
}

impl GraphTempMemory {
    /// Get the recorded sizes of all nodes.
    pub fn node_sizes(&self) -> &NodeSizes {
        &self.node_sizes
    }
}

impl NodeSockets {
    /// The screen position and normal of the input at the given index.
    ///
    /// Returns `None` if there is no input at the given index.
    pub fn input(&self, ix: usize) -> Option<(egui::Pos2, egui::Vec2)> {
        self.inputs
            .get(&ix)
            .map(|&pos| (pos, input_normal(self.flow)))
    }

    /// The screen position and normal of the output at the given index.
    ///
    /// Returns `None` if there is no output at the given index.
    pub fn output(&self, ix: usize) -> Option<(egui::Pos2, egui::Vec2)> {
        self.outputs
            .get(&ix)
            .map(|&pos| (pos, output_normal(self.flow)))
    }

    /// Produces an iterator yielding the index, position, and normal for each input.
    pub fn inputs(&self) -> impl Iterator<Item = (usize, egui::Pos2, egui::Vec2)> + '_ {
        let norm = input_normal(self.flow);
        self.inputs.iter().map(move |(&ix, &pos)| (ix, pos, norm))
    }

    /// Produces an iterator yielding the index, position, and normal for each output.
    pub fn outputs(&self) -> impl Iterator<Item = (usize, egui::Pos2, egui::Vec2)> + '_ {
        let norm = output_normal(self.flow);
        self.outputs.iter().map(move |(&ix, &pos)| (ix, pos, norm))
    }
}

fn input_normal(flow: egui::Direction) -> egui::Vec2 {
    match flow {
        egui::Direction::LeftToRight => egui::Vec2::new(-1.0, 0.0),
        egui::Direction::RightToLeft => egui::Vec2::new(1.0, 0.0),
        egui::Direction::TopDown => egui::Vec2::new(0.0, -1.0),
        egui::Direction::BottomUp => egui::Vec2::new(0.0, 1.0),
    }
}

fn output_normal(flow: egui::Direction) -> egui::Vec2 {
    match flow {
        egui::Direction::LeftToRight => egui::Vec2::new(1.0, 0.0),
        egui::Direction::RightToLeft => egui::Vec2::new(-1.0, 0.0),
        egui::Direction::TopDown => egui::Vec2::new(0.0, 1.0),
        egui::Direction::BottomUp => egui::Vec2::new(0.0, -1.0),
    }
}

impl<'a> Show<'a> {
    /// Instantiate the nodes of the graph.
    pub fn nodes(
        mut self,
        ui: &mut egui::Ui,
        content: impl FnOnce(&mut NodesCtx, &mut egui::Ui),
    ) -> Self {
        {
            let Self {
                graph_id,
                graph_rect,
                selection_rect,
                select,
                socket_press_released,
                ref mut visited,
                ref mut layout,
                immutable,
                ..
            } = self;
            let mut ctx = NodesCtx {
                graph_id,
                graph_rect,
                selection_rect,
                select,
                socket_press_released,
                visited,
                layout,
                immutable,
            };
            content(&mut ctx, ui);
        }
        self
    }

    /// Instantiate the edges of the graph.
    pub fn edges(
        self,
        ui: &mut egui::Ui,
        content: impl FnOnce(&mut EdgesCtx, &mut egui::Ui),
    ) -> Self {
        {
            let Self {
                graph_rect,
                graph_id,
                selection_rect,
                closest_socket,
                immutable,
                ..
            } = self;
            let mut ctx = EdgesCtx {
                graph_id,
                graph_rect,
                selection_rect,
                closest_socket,
                immutable,
            };
            content(&mut ctx, ui);
        }
        self
    }
}

/// If a node didn't appear this update, it's likely because the user has
/// removed the node from their graph, so we should stop tracking it.
fn prune_unused_nodes(graph_id: egui::Id, visited: &HashSet<NodeId>, ui: &mut egui::Ui) {
    let gmem_arc = memory(ui, graph_id);
    let mut gmem = gmem_arc.lock().expect("failed to lock graph temp memory");
    gmem.node_sizes.retain(|k, _| visited.contains(k));
    gmem.selection.nodes.retain(|k| visited.contains(k));
    if let Some(socket) = gmem.closest_socket.as_ref() {
        if !visited.contains(&socket.node) {
            gmem.closest_socket = None;
        }
    }
    if let Some(pressed) = gmem.pressed.as_ref() {
        match pressed.action {
            PressAction::DragNodes {
                node: Some(PressedNode { id: n, .. }),
            }
            | PressAction::Socket(socket::Socket { node: n, .. })
                if !visited.contains(&n) =>
            {
                gmem.pressed = None
            }
            _ => (),
        }
    }
}

impl EdgesCtx {
    /// Retrieves the position and normal of the specified input for the given node.
    ///
    /// Returns `None` if either the `node` or `input` do not exist.
    pub fn input(
        &self,
        ui: &egui::Ui,
        node: NodeId,
        input: usize,
    ) -> Option<(egui::Pos2, egui::Vec2)> {
        let gmem_arc = crate::memory(ui, self.graph_id);
        let gmem = gmem_arc.lock().expect("failed to lock graph temp memory");
        gmem.sockets
            .get(&node)
            .and_then(|sockets| sockets.input(input))
    }

    /// Retrieves the position and normal of the specified output for the given node.
    ///
    /// Returns `None` if either the `node` or `output` do not exist.
    pub fn output(
        &self,
        ui: &egui::Ui,
        node: NodeId,
        output: usize,
    ) -> Option<(egui::Pos2, egui::Vec2)> {
        let gmem_arc = memory(ui, self.graph_id);
        let gmem = gmem_arc.lock().expect("failed to lock graph temp memory");
        gmem.sockets
            .get(&node)
            .and_then(|sockets| sockets.output(output))
    }

    /// If the user is in the progress of creating an edge, this returns the relevant info.
    pub fn in_progress(&self, ui: &egui::Ui) -> Option<EdgeInProgress> {
        let gmem_arc = memory(ui, self.graph_id);
        let gmem = gmem_arc.lock().expect("failed to lock graph temp memory");
        let pressed = gmem.pressed.as_ref()?;
        let start = match pressed.action {
            PressAction::Socket(socket) => {
                let sockets = gmem.sockets.get(&socket.node)?;
                let (pos, normal) = match socket.kind {
                    socket::SocketKind::Input => sockets.input(socket.index)?,
                    socket::SocketKind::Output => sockets.output(socket.index)?,
                };
                socket::PositionedSocket {
                    socket,
                    pos,
                    normal,
                }
            }
            _ => return None,
        };
        let (end_pos, end_socket) = match gmem.closest_socket {
            Some(socket) if socket.kind != start.socket.kind => {
                let sockets = gmem.sockets.get(&socket.node)?;
                let (pos, normal) = match socket.kind {
                    socket::SocketKind::Input => sockets.input(socket.index)?,
                    socket::SocketKind::Output => sockets.output(socket.index)?,
                };
                (pos, Some((socket.kind, normal)))
            }
            _ => (pressed.current_pos, None),
        };
        Some(EdgeInProgress {
            start,
            end_pos,
            end_socket,
        })
    }

    /// The full rect occuppied by the graph widget.
    pub fn graph_rect(&self) -> egui::Rect {
        self.graph_rect
    }
}

pub struct EdgeInProgress {
    /// The socket at the start end of the edge.
    pub start: socket::PositionedSocket,
    /// The end position of the edge in progress.
    ///
    /// If there is no socket within the interaction radius, this will be the pointer position.
    /// Otherwise, this will be the position of the closest socket who's `SocketKind` is opposite
    /// to `start.kind`.
    pub end_pos: egui::Pos2,
    /// The closest socket who's `SocketKind` is opposite to `start.kind`.
    ///
    /// This is `None` in the case that there are no sockets within the interaction radius.
    pub end_socket: Option<(socket::SocketKind, egui::Vec2)>,
}

impl EdgeInProgress {
    /// Construct the bezier curve for this in-progress edge.
    ///
    /// `curvature` is a normalized `0.0..=1.0` value controlling how
    /// pronounced the curve is. See [`bezier::Cubic::DEFAULT_CURVATURE`].
    pub fn bezier_cubic(&self, curvature: f32) -> bezier::Cubic {
        let start = (self.start.pos, self.start.normal);
        let end_normal = self
            .end_socket
            .as_ref()
            .map(|&(_, n)| n)
            .unwrap_or(-self.start.normal);
        let end = (self.end_pos, end_normal);
        bezier::Cubic::from_edge_points(start, end, curvature)
    }

    /// Short-hand for painting the in-progress edge with some reasonable defaults.
    ///
    /// If you require custom styling of the in-progress edge, use
    /// [`EdgeInProgress::bezier_cubic`] or the individual fields to paint it
    /// however you wish.
    pub fn show(&self, ui: &egui::Ui, curvature: f32) {
        let dist_per_pt = crate::edge::Edge::DEFAULT_DISTANCE_PER_POINT;
        let bezier = self.bezier_cubic(curvature);
        let pts = bezier.flatten(dist_per_pt).collect();
        let stroke = ui.visuals().widgets.active.fg_stroke;
        ui.painter().add(egui::Shape::line(pts, stroke));
    }
}

impl Default for View {
    fn default() -> Self {
        Self {
            scene_rect: egui::Rect::ZERO,
            layout: Default::default(),
        }
    }
}

/// Find the socket that is closest to the given point.
///
/// Returns the socket alongside the squared distance from the socket.
fn find_closest_socket(
    pos_graph: egui::Pos2,
    layout: &Layout,
    gmem: &GraphTempMemory,
    ui: &egui::Ui,
) -> Option<(socket::Socket, f32)> {
    // TODO: if we wanted to be super efficient, we could maintain a quadtree of
    // nodes and sockets...
    let mut closest_socket = None;
    let socket_radius = ui
        .spacing()
        .interact_size
        .x
        .min(ui.spacing().interact_size.y);
    let visible_rect = ui.clip_rect();
    let socket_radius_sq = socket_radius * socket_radius;
    for (&n_id, &n_graph) in layout {
        // Only check visible nodes.
        let n_screen = n_graph;
        let size = match gmem.node_sizes.get(&n_id) {
            None => continue,
            Some(&size) => size,
        };
        let rect = egui::Rect::from_min_size(n_screen, size);
        if !visible_rect.intersects(rect) {
            continue;
        }
        let sockets = match gmem.sockets.get(&n_id) {
            None => continue,
            Some(sockets) => sockets,
        };

        // Check inputs.
        for (ix, p, _) in sockets.inputs() {
            let dist_sq = pos_graph.distance_sq(p);
            if dist_sq < socket_radius_sq {
                let socket = socket::Socket {
                    node: n_id,
                    kind: socket::SocketKind::Input,
                    index: ix,
                };
                closest_socket = match closest_socket {
                    None => Some((socket, dist_sq)),
                    Some((_, d_sq)) if dist_sq < d_sq => Some((socket, dist_sq)),
                    _ => closest_socket,
                }
            }
        }

        // Check outputs.
        for (ix, p, _) in sockets.outputs() {
            let dist_sq = pos_graph.distance_sq(p);
            if dist_sq < socket_radius_sq {
                let socket = socket::Socket {
                    node: n_id,
                    kind: socket::SocketKind::Output,
                    index: ix,
                };
                closest_socket = match closest_socket {
                    None => Some((socket, dist_sq)),
                    Some((_, d_sq)) if dist_sq < d_sq => Some((socket, dist_sq)),
                    _ => closest_socket,
                }
            }
        }
    }

    closest_socket
}

/// Interpret some basic interactions from the state of the graph and recent input.
fn graph_interaction(
    layout: &Layout,
    pointer: &egui::PointerState,
    closest_socket: Option<socket::Socket>,
    ptr_on_graph: bool,
    ptr_graph: egui::Pos2,
    pressed: Option<&Pressed>,
) -> GraphInteraction {
    let mut select = false;
    let mut socket_press_released = None;
    let mut drag_nodes_delta = egui::Vec2::ZERO;
    let mut selection_rect = None;

    // Check for selecting/dragging.
    let pressed: Option<Pressed> = if let Some(pressed) = pressed {
        match pressed.action {
            PressAction::DragNodes {
                node: Some(ref node),
            } => {
                // Determine the drag delta.
                let delta = ptr_graph - pressed.origin_pos;
                let target = node.position_at_origin + delta;
                if let Some(current) = layout.get(&node.id) {
                    drag_nodes_delta = target - *current;
                }
            }
            PressAction::Select => {
                let min = pressed.origin_pos;
                let max = ptr_graph;
                selection_rect = Some(egui::Rect::from_two_pos(min, max));
            }
            _ => (),
        }

        // The press action has ended.
        if pointer.primary_released() {
            match pressed.action {
                PressAction::Select => select = true,
                PressAction::Socket(socket) => socket_press_released = Some(socket),
                _ => (),
            }
            None
        } else {
            Some(Pressed {
                current_pos: ptr_graph,
                ..pressed.clone()
            })
        }
    // Check for the beginning of a socket press or rectangular selection.
    } else if ptr_on_graph
        && pointer.button_down(egui::PointerButton::Primary)
        && pointer.button_pressed(egui::PointerButton::Primary)
    {
        // Choose which press action based on whether or not a socket was pressed.
        let action = match closest_socket {
            Some(socket) => PressAction::Socket(socket),
            None => {
                let min = ptr_graph;
                let max = ptr_graph;
                selection_rect = Some(egui::Rect::from_two_pos(min, max));
                PressAction::Select
            }
        };

        let pressed = Pressed {
            over_selection_at_origin: false,
            origin_pos: ptr_graph,
            current_pos: ptr_graph,
            action,
        };
        Some(pressed)

    // Otherwise, pass through existing state.
    } else {
        pressed.cloned()
    };

    GraphInteraction {
        pressed,
        socket_press_released,
        select,
        selection_rect,
        drag_nodes_delta,
    }
}

// Paint a subtle dot grid to check camera movement.
fn paint_dot_grid(visible_rect: egui::Rect, ui: &mut egui::Ui) {
    let dot_step = ui.spacing().interact_size.y;
    let vis = ui.style().noninteractive();
    let x_dots = (visible_rect.min.x / dot_step) as i32..=(visible_rect.max.x / dot_step) as i32;
    let y_dots = (visible_rect.min.y / dot_step) as i32..=(visible_rect.max.y / dot_step) as i32;
    for x_dot in x_dots {
        for y_dot in y_dots.clone() {
            let x = x_dot as f32 * dot_step;
            let y = y_dot as f32 * dot_step;
            let r = egui::Rect::from_center_size([x, y].into(), [1.0; 2].into());
            let color = vis.bg_stroke.color;
            ui.painter().circle_filled(r.center(), 0.5, color);
        }
    }
}

// Paint the background rect.
fn paint_background(visible_rect: egui::Rect, ui: &mut egui::Ui) {
    let vis = ui.style().noninteractive();
    let stroke = egui::Stroke {
        width: 0.0,
        ..vis.bg_stroke
    };
    let fill = vis.bg_fill;
    ui.painter()
        .rect(visible_rect, 0.0, fill, stroke, egui::StrokeKind::Inside);
}

/// Paint the selection area rectangle.
fn paint_selection_area(sel_rect: egui::Rect, ui: &mut egui::Ui) {
    let color = ui.visuals().weak_text_color();
    let fill = color.linear_multiply(0.125);
    let width = 1.0;
    let stroke = egui::Stroke { width, color };
    ui.painter()
        .rect(sel_rect, 0.0, fill, stroke, egui::StrokeKind::Inside);
}

/// Combines the given id src with the `TypeId` of the `Graph` to produce a unique `egui::Id`.
pub fn id(id_src: impl Hash) -> egui::Id {
    egui::Id::new((std::any::TypeId::of::<Graph>(), id_src))
}

/// Access the graph's temporary memory for the given graph ID.
///
/// This allows reading graph state like node sizes without cloning.
/// If no memory exists for the graph ID, a default GraphTempMemory is created and stored.
pub fn with_graph_memory<R>(
    ctx: &egui::Context,
    graph_id: egui::Id,
    f: impl FnOnce(&GraphTempMemory) -> R,
) -> R {
    let gmem_arc = ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Arc<Mutex<GraphTempMemory>>>(graph_id)
            .clone()
    });
    let gmem = gmem_arc.lock().expect("failed to lock graph temp memory");
    f(&gmem)
}

/// Checks if a node with the given ID is currently selected in the specified graph.
pub fn is_node_selected(ui: &egui::Ui, graph_id: egui::Id, node_id: NodeId) -> bool {
    let gmem_arc = memory(ui, graph_id);
    let gmem = gmem_arc.lock().expect("failed to lock graph temp memory");
    gmem.selection.nodes.contains(&node_id)
}

/// Short-hand for retrieving access to the graph's temporary memory from the `Ui`.
fn memory(ui: &egui::Ui, graph_id: egui::Id) -> Arc<Mutex<GraphTempMemory>> {
    ui.ctx().data_mut(|d| {
        d.get_temp_mut_or_default::<Arc<Mutex<GraphTempMemory>>>(graph_id)
            .clone()
    })
}
