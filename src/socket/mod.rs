pub mod layout;

use crate::node::NodeId;

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

/// Collected [`egui::Response`]s for all sockets on a node.
///
/// Each socket is allocated as an interactive widget (with [`egui::Sense::hover`]),
/// enabling standard egui interactions like tooltips and hover detection.
pub struct SocketResponses {
    inputs: std::collections::BTreeMap<usize, egui::Response>,
    outputs: std::collections::BTreeMap<usize, egui::Response>,
}

impl SocketResponses {
    /// The response for the input socket at the given index.
    pub fn input(&self, ix: usize) -> Option<&egui::Response> {
        self.inputs.get(&ix)
    }

    /// The response for the output socket at the given index.
    pub fn output(&self, ix: usize) -> Option<&egui::Response> {
        self.outputs.get(&ix)
    }

    /// Iterator over all input socket responses, yielding `(index, response)`.
    pub fn inputs(&self) -> impl Iterator<Item = (usize, &egui::Response)> {
        self.inputs.iter().map(|(&ix, r)| (ix, r))
    }

    /// Iterator over all output socket responses, yielding `(index, response)`.
    pub fn outputs(&self) -> impl Iterator<Item = (usize, &egui::Response)> {
        self.outputs.iter().map(|(&ix, r)| (ix, r))
    }
}

/// Paint and interact with all sockets for a node.
///
/// Phase A: extracts highlight state (pressed/closest socket) from graph memory, then drops the lock.
/// Phase B: creates a socket sublayer, paints each socket circle (with highlight), and calls
/// `ui.interact()` to produce per-socket responses.
pub(crate) fn show(
    ui: &mut egui::Ui,
    graph_id: egui::Id,
    node_id: NodeId,
    egui_id: egui::Id,
    frame_layer: egui::LayerId,
    frame_rect: egui::Rect,
    node_sockets: &crate::NodeSockets,
    socket_color: egui::Color32,
    socket_radius: f32,
) -> SocketResponses {
    // Phase A: Store resolved sockets and extract highlight state, then drop the lock.
    let (pressed_socket, closest_socket) = if !node_sockets.inputs.is_empty()
        || !node_sockets.outputs.is_empty()
    {
        let gmem_arc = crate::memory(ui, graph_id);
        let mut gmem = gmem_arc.lock().expect("failed to lock graph temp memory");
        gmem.sockets.insert(node_id, node_sockets.clone());

        let pressed_socket = gmem
            .pressed
            .as_ref()
            .and_then(|pressed| match pressed.action {
                crate::PressAction::Socket(socket) if socket.node == node_id => {
                    Some((socket.kind, socket.index))
                }
                _ => None,
            });

        let closest_socket = match gmem.closest_socket {
            Some(closest) if closest.node == node_id => {
                match gmem.pressed.as_ref().map(|p| &p.action) {
                    Some(crate::PressAction::Socket(socket)) if closest.kind == socket.kind => None,
                    _ => Some((closest.kind, closest.index)),
                }
            }
            _ => None,
        };

        (pressed_socket, closest_socket)
    } else {
        (None, None)
    };

    // Phase B: Create socket layer, interact and paint each socket.
    let socket_layer_id = egui::LayerId::new(frame_layer.order, egui_id.with("sockets"));
    ui.ctx().set_sublayer(frame_layer, socket_layer_id);
    if let Some(transform) = ui.ctx().layer_transform_to_global(frame_layer) {
        ui.ctx().set_transform_layer(socket_layer_id, transform);
    }

    let hl_size = (socket_radius + 4.0).max(4.0);
    let interact_diameter = ui
        .spacing()
        .interact_size
        .x
        .min(ui.spacing().interact_size.y);

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

    let builder = egui::UiBuilder::new()
        .max_rect(frame_rect.expand(hl_size))
        .layer_id(socket_layer_id);

    let mut input_responses = std::collections::BTreeMap::new();
    let mut output_responses = std::collections::BTreeMap::new();

    ui.scope_builder(builder, |ui| {
        let painter = ui.painter();
        for (ix, pos, _) in node_sockets.inputs() {
            if paint_highlight(SocketKind::Input, ix) {
                painter.circle_filled(pos, hl_size, socket_color.linear_multiply(0.25));
            }
            painter.circle_filled(pos, socket_radius, socket_color);
            let id = egui_id.with("in").with(ix);
            let rect = egui::Rect::from_center_size(pos, egui::Vec2::splat(interact_diameter));
            input_responses.insert(ix, ui.interact(rect, id, egui::Sense::hover()));
        }
        for (ix, pos, _) in node_sockets.outputs() {
            if paint_highlight(SocketKind::Output, ix) {
                painter.circle_filled(pos, hl_size, socket_color.linear_multiply(0.25));
            }
            painter.circle_filled(pos, socket_radius, socket_color);
            let id = egui_id.with("out").with(ix);
            let rect = egui::Rect::from_center_size(pos, egui::Vec2::splat(interact_diameter));
            output_responses.insert(ix, ui.interact(rect, id, egui::Sense::hover()));
        }
    });

    SocketResponses {
        inputs: input_responses,
        outputs: output_responses,
    }
}
