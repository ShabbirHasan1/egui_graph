use crate::NodesCtx;
use std::hash::Hash;
use std::ops::{Deref, DerefMut};

/// Derive the `egui::Id` used internally for a node's egui state (layers, area_rect, etc).
pub fn egui_id(graph_id: egui::Id, node_id: NodeId) -> egui::Id {
    graph_id.with(node_id.0)
}

/// The default node widget.
///
/// A `Node` is a thin wrapper around a `Window` and allows for instantiating arbitrary widgets
/// internally.
pub struct Node {
    id: NodeId,
    frame: Option<egui::Frame>,
    inputs: usize,
    outputs: usize,
    flow: egui::Direction,
    socket_radius: f32,
    socket_color: Option<egui::Color32>,
    max_width: Option<f32>,
    animation_time: f32,
}

/// A unique identifier for a node within a graph.
///
/// This is decoupled from `egui::Id` to allow node identity to remain stable
/// even when the graph's egui ID context changes (e.g., when viewing the same
/// graph through different heads or paths).
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct NodeId(pub u64);

/// Describes either an input or output.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SocketKind {
    Input,
    Output,
}

/// Uniquely identifies a socket.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Socket {
    /// The node that owns this socket.
    pub node: NodeId,
    /// Whether the socket is an input or output.
    pub kind: SocketKind,
    /// The index of the socket of this kind.
    pub index: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct PositionedSocket {
    pub socket: Socket,
    /// Screen-space position of the socket.
    pub pos: egui::Pos2,
    /// The normal of the edge along which this socket resides.
    pub normal: egui::Vec2,
}

/// An extension around the `egui::Response` that indicates node selection.
pub struct NodeResponse {
    response: egui::Response,
    selection_changed: bool,
    selected: bool,
    removed: bool,
    /// Some event occurred related to the creation of an edge.
    edge_event: Option<EdgeEvent>,
}

/// Events related to the creation of an edge to or from a node.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EdgeEvent {
    /// Occurs if a socket was just pressed to start creating an edge.
    Started { kind: SocketKind, index: usize },
    /// Occurs when the primary mouse button was released creating an edge ending at the specified
    /// socket.
    Ended { kind: SocketKind, index: usize },
    /// If there was an edge in progress starting from this node, this indicates that it was
    /// cancelled.
    Cancelled,
}

impl Node {
    /// Begin instantiating a new node widget.
    pub fn new(id_src: impl Hash) -> Self {
        Self::from_id(NodeId::new(id_src))
    }

    /// Construct the node directly from its `NodeId`.
    pub fn from_id(id: NodeId) -> Self {
        Self {
            id,
            frame: None,
            max_width: None,
            socket_color: None,
            inputs: 0,
            outputs: 0,
            flow: egui::Direction::LeftToRight,
            socket_radius: 3.0,
            animation_time: 0.1,
        }
    }

    /// Specify the `Frame` used for the `Node`'s window.
    ///
    /// The default is retrieved via `egui_graph::node::default_frame(ui.style())`.
    pub fn frame(mut self, frame: egui::Frame) -> Self {
        self.frame = Some(frame);
        self
    }

    /// Optionally specify the max width of the `Node`'s window.
    ///
    /// By default, `ui.spacing().text_edit_width` is used.
    pub fn max_width(mut self, w: f32) -> Self {
        self.max_width = Some(w);
        self
    }

    pub fn inputs(mut self, n: usize) -> Self {
        self.inputs = n;
        self
    }

    pub fn outputs(mut self, n: usize) -> Self {
        self.outputs = n;
        self
    }

    /// The direction of dataflow through the graph.
    ///
    /// This determines which edges the inputs and outputs are distributed across.
    ///
    /// E.g. `LeftToRight` will have inputs on the left and outputs on the right.
    ///
    /// On vertical edges, inputs/outputs start at the top and end at the bottom.
    ///
    /// On horizontal edges, inputs/outputs start at the left and end at the right.
    ///
    /// Default direction is `LeftToRight`.
    pub fn flow(mut self, flow: egui::Direction) -> Self {
        self.flow = flow;
        self
    }

    /// The color of the input and output sockets.
    pub fn socket_color(mut self, color: egui::Color32) -> Self {
        self.socket_color = Some(color);
        self
    }

    /// The radius of the input and output sockets.
    pub fn socket_radius(mut self, radius: f32) -> Self {
        self.socket_radius = radius;
        self
    }

    /// The time taken (seconds) for the node to interpolate toward a new location.
    ///
    /// Default: `0.1`.
    pub fn animation_time(mut self, time: f32) -> Self {
        self.animation_time = time;
        self
    }

    /// Present the `Node`'s `Window` and add the given contents.
    pub fn show(
        self,
        ctx: &mut NodesCtx,
        ui: &mut egui::Ui,
        content: impl FnOnce(&mut egui::Ui),
    ) -> NodeResponse {
        self.show_impl(ctx, ui, Box::new(content) as Box<_>)
    }

    fn show_impl<'a>(
        self,
        ctx: &mut NodesCtx,
        ui: &mut egui::Ui,
        content: Box<dyn FnOnce(&mut egui::Ui) + 'a>,
    ) -> NodeResponse {
        let layout = &mut ctx.layout;

        // Indicate that we've visited this node this update.
        ctx.visited.insert(self.id);

        // Determine the current position of the window relative to the graph origin.
        let target_pos_graph = layout.entry(self.id).or_insert_with(|| {
            // If the mouse is over the graph, add the node under the mouse.
            // Otherwise, add the node to the top-left.
            let clip_rect = ui.clip_rect();
            let mut pos = clip_rect.center();
            if ui.rect_contains_pointer(clip_rect) {
                if let Some(ptr) = ui.response().hover_pos() {
                    pos = ptr;
                }
            }
            egui::Pos2::new(pos.x, pos.y)
        });

        // Interpolate toward the desired position over time for auto-layout.
        // Only do so if this node is not selected with the primary mouse down.
        let is_selected = crate::is_node_selected(ui, ctx.graph_id, self.id);
        let is_primary_down = ui.input(|i| i.pointer.primary_down());
        let animation_time = if is_selected && is_primary_down {
            0.0
        } else {
            self.animation_time
        };
        let pos_graph = {
            let idx = ctx.graph_id.with(self.id.0).with("x");
            let idy = ctx.graph_id.with(self.id.0).with("y");
            let ctx = ui.ctx();
            let x = ctx.animate_value_with_time(idx, target_pos_graph.x, animation_time);
            let y = ctx.animate_value_with_time(idy, target_pos_graph.y, animation_time);
            egui::Pos2::new(x, y)
        };

        // The window should always be at least the interaction size.
        let min_item_spacing = ui.spacing().item_spacing.x.min(ui.spacing().item_spacing.y);
        let min_interact_len = ui
            .spacing()
            .interact_size
            .x
            .min(ui.spacing().interact_size.y);
        let mut min_size = egui::Vec2::splat(min_interact_len);
        // However, it should also always be at least large enough to comfortably show all
        // inlets/outlets.
        let max_sockets = std::cmp::max(self.inputs, self.outputs);
        let min_socket_gap = min_interact_len + min_item_spacing;
        let win_corner_radius = ui.visuals().window_corner_radius.ne as f32;
        let socket_padding = win_corner_radius + min_interact_len * 0.5;
        let min_len = (max_sockets.max(1) - 1) as f32 * min_socket_gap + socket_padding * 2.0;
        if max_sockets > 1 {
            match self.flow {
                egui::Direction::LeftToRight | egui::Direction::RightToLeft => {
                    min_size.y = min_size.y.max(min_len);
                }
                egui::Direction::TopDown | egui::Direction::BottomUp => {
                    min_size.x = min_size.x.max(min_len);
                }
            }
        }
        // Retrieve the frame.
        let mut frame = self.frame.unwrap_or_else(|| default_frame(ui.style()));

        let max_w = self.max_width.unwrap_or(ui.spacing().text_edit_width);
        let max_size = egui::Vec2::new(max_w, ctx.graph_rect.height());

        // Track changes in selection for the node response.
        let mut selection_changed = false;

        // Determine whether or not this node is within the selection rect.
        // If `shift` is down, rectangle selection is reserved for edges.
        // NOTE: We use the size from last frame as we don't know the size until
        // the user's content is added... Is there a better way to handle this?
        let (mut selected, in_selection_rect) = {
            let gmem_arc = crate::memory(ui, ctx.graph_id);
            let mut gmem = gmem_arc.lock().expect("failed to lock graph temp memory");
            let in_selection_rect = match ctx.selection_rect {
                Some(sel_rect) if ui.input(|i| !i.modifiers.shift) => {
                    let size = gmem
                        .node_sizes
                        .get(&self.id)
                        .cloned()
                        .unwrap_or(egui::Vec2::ZERO);
                    let rect = egui::Rect::from_min_size(pos_graph, size);
                    sel_rect.intersects(rect)
                }
                _ => false,
            };

            // Update the selection if the primary mouse button was just released.
            if ctx.select {
                if in_selection_rect && ui.input(|i| !i.modifiers.shift) {
                    if gmem.selection.nodes.insert(self.id) {
                        selection_changed = true;
                    }
                } else if !ui.input(|i| i.modifiers.ctrl) {
                    if gmem.selection.nodes.remove(&self.id) {
                        selection_changed = true;
                    }
                }
            }

            let selected = gmem.selection.nodes.contains(&self.id);

            (selected, in_selection_rect)
        };

        // Style the frame based on interaction.
        frame.stroke.width = ui.visuals().selection.stroke.width;
        if selected {
            frame.stroke = ui.visuals().selection.stroke;
        } else if in_selection_rect {
            let color = ui.visuals().weak_text_color();
            frame.shadow.color = color;
            frame.stroke = ui.visuals().selection.stroke;
            frame.stroke.color = color;
        }

        // Custom framed node container that remains in the scene's layer
        let put_size = egui::Vec2::new(max_size.x, min_size.y);
        let put_rect = egui::Rect::from_min_size(pos_graph, put_size);

        // Put the node's frame on a layer above the scene's UI layer.
        let scene_layer = ui.layer_id();
        let frame_layer = egui::LayerId::new(scene_layer.order, ctx.graph_id.with(self.id.0));
        ui.ctx().set_sublayer(scene_layer, frame_layer);
        if let Some(transform) = ui.ctx().layer_transform_to_global(scene_layer) {
            ui.ctx().set_transform_layer(frame_layer, transform);
        }

        // A `Ui` scope for placing the `Frame`.
        let builder = egui::UiBuilder::new()
            .max_rect(put_rect)
            .layer_id(frame_layer)
            .sense(egui::Sense::click_and_drag());
        let inner_response = ui.scope_builder(builder, |ui| {
            // Show the frame.
            let inner_response = frame.show(ui, |ui| {
                // Create a node content UI that can be clicked and dragged.
                let builder = egui::UiBuilder::new().sense(egui::Sense::click_and_drag());
                let inner_response = ui.scope_builder(builder, |ui| {
                    // Set the minimum size required to layout the sockets.
                    let gap = egui::Vec2::splat(win_corner_radius * 2.0);
                    let min_size = min_size - gap;
                    ui.set_min_size(min_size);

                    // Set the user's content.
                    content(ui);
                });
                inner_response.response
            });

            // Merge the content area response with the frame response.
            inner_response.response.union(inner_response.inner)
        });
        let mut response = inner_response.response.union(inner_response.inner);

        // Update the stored data for this node and check for edge events.
        let mut edge_event = None;
        {
            let gmem_arc = crate::memory(ui, ctx.graph_id);
            let mut gmem = gmem_arc.lock().expect("failed to lock graph temp memory");
            gmem.node_sizes.insert(self.id, response.rect.size());

            let ctrl_down = ui.input(|i| i.modifiers.ctrl);

            // If the window is pressed, select the node.
            let pointer = &ui.input(|i| i.pointer.clone());
            if response.is_pointer_button_down_on() && pointer.primary_pressed() {
                // If ctrl is down, check for deselection.
                let was_selected = gmem.selection.nodes.contains(&self.id);
                if ctrl_down && was_selected {
                    selection_changed = gmem.selection.nodes.remove(&self.id);
                    selected = false;
                } else {
                    // Clear other selections if ctrl is not pressed and this is newly pressed.
                    if !ctrl_down && !was_selected {
                        gmem.selection.nodes.clear();
                    }
                    selection_changed = gmem.selection.nodes.insert(self.id);
                    selected = true;
                    // We must initialize gmem.pressed here so that subsequent drag updates work correctly.
                    if gmem.pressed.is_none() {
                        let ptr_graph = response.hover_pos().unwrap_or_default();
                        gmem.pressed = Some(crate::Pressed {
                            over_selection_at_origin: true,
                            origin_pos: ptr_graph,
                            current_pos: ptr_graph,
                            action: crate::PressAction::DragNodes {
                                node: Some(crate::PressedNode {
                                    id: self.id,
                                    position_at_origin: pos_graph,
                                }),
                            },
                        });
                    }
                }

            // If the primary button was pressed, check for edge events.
            } else if !response.is_pointer_button_down_on() && pointer.primary_pressed() {
                // If this node's socket was pressed, create a start event.
                if let Some(ref pressed) = gmem.pressed {
                    if let crate::PressAction::Socket(socket) = pressed.action {
                        if self.id == socket.node {
                            let kind = socket.kind;
                            let index = socket.index;
                            edge_event = Some(EdgeEvent::Started { kind, index });
                        }
                    }
                }

                // Also check for deselection.
                if !ctrl_down
                    && !gmem
                        .pressed
                        .as_ref()
                        .map(|p| p.over_selection_at_origin)
                        .unwrap_or(false)
                    && !gmem.selection.nodes.contains(&self.id)
                {
                    selection_changed = gmem.selection.nodes.remove(&self.id);
                    selected = false;
                }

            // Check for edge creation / cancellation events.
            } else if let Some(r) = ctx.socket_press_released {
                if let Some(c) = gmem.closest_socket {
                    if r.kind == c.kind && self.id == r.node {
                        edge_event = Some(EdgeEvent::Cancelled);
                    } else if self.id == c.node && ui.input(|i| i.pointer.primary_released()) {
                        let kind = c.kind;
                        let index = c.index;
                        edge_event = Some(EdgeEvent::Ended { kind, index });
                    }
                } else if edge_event.is_none() {
                    if self.id == r.node {
                        edge_event = Some(EdgeEvent::Cancelled);
                    }
                }
            }
        }

        // The inlets/outlets.
        if self.inputs > 0 || self.outputs > 0 {
            let in_gap = |len: f32| {
                if self.inputs > 1 {
                    len / (self.inputs - 1) as f32
                } else {
                    0.0
                }
            };
            let out_gap = |len: f32| {
                if self.outputs > 1 {
                    len / (self.outputs - 1) as f32
                } else {
                    0.0
                }
            };
            let (mut in_pos, mut out_pos, in_step, out_step) = match self.flow {
                egui::Direction::LeftToRight => {
                    let len = response.rect.height() - socket_padding * 2.0;
                    let in_step = egui::Vec2::new(0.0, in_gap(len));
                    let out_step = egui::Vec2::new(0.0, out_gap(len));
                    let start = response.rect.min.y + socket_padding;
                    let in_pos = egui::Pos2::new(response.rect.min.x, start);
                    let out_pos = egui::Pos2::new(response.rect.max.x, start);
                    (in_pos, out_pos, in_step, out_step)
                }
                egui::Direction::RightToLeft => {
                    let len = response.rect.height() - socket_padding * 2.0;
                    let in_step = egui::Vec2::new(0.0, in_gap(len));
                    let out_step = egui::Vec2::new(0.0, out_gap(len));
                    let start = response.rect.min.y + socket_padding;
                    let in_pos = egui::Pos2::new(response.rect.max.x, start);
                    let out_pos = egui::Pos2::new(response.rect.min.x, start);
                    (in_pos, out_pos, in_step, out_step)
                }
                egui::Direction::TopDown => {
                    let len = response.rect.width() - socket_padding * 2.0;
                    let in_step = egui::Vec2::new(in_gap(len), 0.0);
                    let out_step = egui::Vec2::new(out_gap(len), 0.0);
                    let start = response.rect.min.x + socket_padding;
                    let in_pos = egui::Pos2::new(start, response.rect.min.y);
                    let out_pos = egui::Pos2::new(start, response.rect.max.y);
                    (in_pos, out_pos, in_step, out_step)
                }
                egui::Direction::BottomUp => {
                    let len = response.rect.width() - socket_padding * 2.0;
                    let in_step = egui::Vec2::new(in_gap(len), 0.0);
                    let out_step = egui::Vec2::new(out_gap(len), 0.0);
                    let start = response.rect.min.x + socket_padding;
                    let in_pos = egui::Pos2::new(start, response.rect.max.y);
                    let out_pos = egui::Pos2::new(start, response.rect.min.y);
                    (in_pos, out_pos, in_step, out_step)
                }
            };

            // Store the layout of the sockets for edge instantiation.
            let sockets = crate::NodeSockets {
                flow: self.flow,
                input: crate::Sockets {
                    count: self.inputs,
                    start: in_pos,
                    step: in_step,
                },
                output: crate::Sockets {
                    count: self.outputs,
                    start: out_pos,
                    step: out_step,
                },
            };
            let gmem_arc = crate::memory(ui, ctx.graph_id);
            let mut gmem = gmem_arc.lock().expect("failed to lock graph temp memory");
            gmem.sockets.insert(self.id, sockets);

            // Check whether or not this node has a pressed socket.
            let pressed_socket = gmem
                .pressed
                .as_ref()
                .and_then(|pressed| match pressed.action {
                    crate::PressAction::Socket(socket) if socket.node == self.id => {
                        Some((socket.kind, socket.index))
                    }
                    _ => None,
                });

            // Check if the mouse is closest to one of this node's sockets.
            let closest_socket = match gmem.closest_socket {
                Some(closest) if closest.node == self.id => {
                    // If there is a pressed socket, only highlight if this socket is of the
                    // opposite kind.
                    match gmem.pressed.as_ref().map(|p| &p.action) {
                        Some(crate::PressAction::Socket(socket)) if closest.kind == socket.kind => {
                            None
                        }
                        _ => Some((closest.kind, closest.index)),
                    }
                }
                _ => None,
            };

            // Whether or not to paint the highlight.
            let paint_highlight = |kind, ix| {
                if let Some((k, i)) = pressed_socket {
                    if k == kind && i == ix {
                        return true;
                    }
                }
                if let Some((k, i)) = closest_socket {
                    if k == kind && i == ix {
                        return true;
                    }
                }
                false
            };

            let color = self.socket_color.unwrap_or(ui.visuals().text_color());
            let hl_size = (self.socket_radius + 4.0).max(4.0);
            let painter = ui.painter();
            for ix in 0..self.inputs {
                if paint_highlight(SocketKind::Input, ix) {
                    painter.circle_filled(in_pos, hl_size, color.linear_multiply(0.25));
                }
                painter.circle_filled(in_pos, self.socket_radius, color);
                in_pos += in_step;
            }
            for ix in 0..self.outputs {
                if paint_highlight(SocketKind::Output, ix) {
                    painter.circle_filled(out_pos, hl_size, color.linear_multiply(0.25));
                }
                painter.circle_filled(out_pos, self.socket_radius, color);
                out_pos += out_step;
            }
        }

        // If the delete or backspace key was pressed and the node is selected, remove it.
        let removed = if selected
            && !ui.ctx().wants_keyboard_input()
            && ui.input(|i| i.key_pressed(egui::Key::Delete) | i.key_pressed(egui::Key::Backspace))
        {
            // Remove ourselves from the selection.
            let gmem_arc = crate::memory(ui, ctx.graph_id);
            let mut gmem = gmem_arc.lock().expect("failed to lock graph temp memory");
            selection_changed = gmem.selection.nodes.remove(&self.id);
            selected = false;
            true
        } else {
            false
        };

        if selection_changed || removed || edge_event.is_some() {
            response.mark_changed();
        }

        NodeResponse {
            response,
            selection_changed,
            selected,
            removed,
            edge_event,
        }
    }
}

impl NodeId {
    /// Create a new NodeId by hashing any hashable value.
    pub fn new(id_src: impl Hash) -> Self {
        use std::hash::Hasher;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        id_src.hash(&mut hasher);
        NodeId(hasher.finish())
    }

    /// Create a NodeId directly from a u64.
    pub const fn from_u64(v: u64) -> Self {
        NodeId(v)
    }

    /// Get the raw u64 value.
    pub const fn value(self) -> u64 {
        self.0
    }
}

impl From<u64> for NodeId {
    fn from(v: u64) -> Self {
        NodeId(v)
    }
}

impl NodeResponse {
    /// Whether or not the selection changed and, if so, whether or not the node is now selected.
    pub fn selection(&self) -> Option<bool> {
        if self.selection_changed {
            Some(self.selected)
        } else {
            None
        }
    }

    /// Whether or not the node is selected.
    pub fn selected(&self) -> bool {
        self.selected
    }

    /// Whether or not the node was removed from the graph.
    ///
    /// This occurs if the `Delete` key is pressed while the node is selected.
    pub fn removed(&self) -> bool {
        self.removed
    }

    /// Consume the node response and produce the inner `egui::Response`.
    pub fn into_inner(self) -> egui::Response {
        self.response
    }

    /// Whether or not any events occurred related to the creation of an edge.
    pub fn edge_event(&self) -> Option<EdgeEvent> {
        self.edge_event
    }
}

impl Deref for NodeResponse {
    type Target = egui::Response;
    fn deref(&self) -> &Self::Target {
        &self.response
    }
}

impl DerefMut for NodeResponse {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.response
    }
}

/// The default frame styling used for the `Node`'s `Window`.
pub fn default_frame(style: &egui::Style) -> egui::Frame {
    let mut frame = egui::Frame::window(style);
    frame.shadow.offset = [2, 2];
    frame.shadow.spread = (frame.shadow.spread as f32 * 0.25) as u8;
    frame.stroke.width = 0.0;
    frame
}
