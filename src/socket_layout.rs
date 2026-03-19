use crate::NodeSockets;
use std::collections::BTreeMap;

/// Controls how socket positions are determined for a node.
///
/// Pre-initialized as `Auto` from the `Node` builder's input/output counts.
/// Users can ignore it for automatic spacing, or switch to explicit positioning
/// via [`SocketLayout::input`], [`SocketLayout::output`], or [`SocketLayout::row`].
pub struct SocketLayout {
    flow: egui::Direction,
    inputs: SocketPositions,
    outputs: SocketPositions,
}

enum SocketPositions {
    /// Evenly spaced along the edge (current behavior).
    Auto(usize),
    /// Explicit cross-axis positions. Only positioned sockets are rendered.
    Explicit(BTreeMap<usize, f32>),
}

impl SocketLayout {
    /// Create a new `SocketLayout` in auto mode with the given socket counts.
    pub fn auto(flow: egui::Direction, inputs: usize, outputs: usize) -> Self {
        Self {
            flow,
            inputs: SocketPositions::Auto(inputs),
            outputs: SocketPositions::Auto(outputs),
        }
    }

    /// Register an input socket aligned with the cross-axis center of `rect`.
    ///
    /// Switches to explicit positioning on first call.
    pub fn input(&mut self, ix: usize, rect: egui::Rect) {
        let cross = cross_axis_center(self.flow, rect);
        self.inputs.set_explicit(ix, cross);
    }

    /// Register an output socket aligned with the cross-axis center of `rect`.
    ///
    /// Switches to explicit positioning on first call.
    pub fn output(&mut self, ix: usize, rect: egui::Rect) {
        let cross = cross_axis_center(self.flow, rect);
        self.outputs.set_explicit(ix, cross);
    }

    /// Render content in a `ui.scope`, registering sockets aligned with its
    /// cross-axis center.
    pub fn row<R>(
        &mut self,
        ui: &mut egui::Ui,
        input: Option<usize>,
        output: Option<usize>,
        content: impl FnOnce(&mut egui::Ui) -> R,
    ) -> egui::InnerResponse<R> {
        let ir = ui.scope(content);
        if let Some(ix) = input {
            self.input(ix, ir.response.rect);
        }
        if let Some(ix) = output {
            self.output(ix, ir.response.rect);
        }
        ir
    }

    /// Resolve the layout into concrete `NodeSockets` given the final frame
    /// rect and socket padding.
    pub(crate) fn resolve(
        self,
        flow: egui::Direction,
        rect: egui::Rect,
        socket_padding: f32,
    ) -> NodeSockets {
        let inputs = resolve_positions(&self.inputs, flow, rect, socket_padding, true);
        let outputs = resolve_positions(&self.outputs, flow, rect, socket_padding, false);
        NodeSockets {
            flow,
            inputs,
            outputs,
        }
    }
}

impl SocketPositions {
    fn set_explicit(&mut self, ix: usize, cross: f32) {
        match self {
            SocketPositions::Auto(_) => {
                let mut map = BTreeMap::new();
                map.insert(ix, cross);
                *self = SocketPositions::Explicit(map);
            }
            SocketPositions::Explicit(map) => {
                map.insert(ix, cross);
            }
        }
    }
}

/// The cross-axis center of a rect relative to the flow direction.
fn cross_axis_center(flow: egui::Direction, rect: egui::Rect) -> f32 {
    match flow {
        egui::Direction::LeftToRight | egui::Direction::RightToLeft => rect.center().y,
        egui::Direction::TopDown | egui::Direction::BottomUp => rect.center().x,
    }
}

/// Resolve a `SocketPositions` into a `BTreeMap<usize, Pos2>`.
fn resolve_positions(
    positions: &SocketPositions,
    flow: egui::Direction,
    rect: egui::Rect,
    socket_padding: f32,
    is_input: bool,
) -> BTreeMap<usize, egui::Pos2> {
    match positions {
        SocketPositions::Auto(count) => resolve_auto(*count, flow, rect, socket_padding, is_input),
        SocketPositions::Explicit(map) => resolve_explicit(map, flow, rect, is_input),
    }
}

/// Auto mode: evenly space sockets along the edge (existing behavior).
fn resolve_auto(
    count: usize,
    flow: egui::Direction,
    rect: egui::Rect,
    socket_padding: f32,
    is_input: bool,
) -> BTreeMap<usize, egui::Pos2> {
    let mut result = BTreeMap::new();
    if count == 0 {
        return result;
    }
    let gap = |len: f32| {
        if count > 1 {
            len / (count - 1) as f32
        } else {
            0.0
        }
    };
    let (start, step) = match flow {
        egui::Direction::LeftToRight => {
            let len = rect.height() - socket_padding * 2.0;
            let main_x = if is_input { rect.min.x } else { rect.max.x };
            let start = egui::Pos2::new(main_x, rect.min.y + socket_padding);
            let step = egui::Vec2::new(0.0, gap(len));
            (start, step)
        }
        egui::Direction::RightToLeft => {
            let len = rect.height() - socket_padding * 2.0;
            let main_x = if is_input { rect.max.x } else { rect.min.x };
            let start = egui::Pos2::new(main_x, rect.min.y + socket_padding);
            let step = egui::Vec2::new(0.0, gap(len));
            (start, step)
        }
        egui::Direction::TopDown => {
            let len = rect.width() - socket_padding * 2.0;
            let main_y = if is_input { rect.min.y } else { rect.max.y };
            let start = egui::Pos2::new(rect.min.x + socket_padding, main_y);
            let step = egui::Vec2::new(gap(len), 0.0);
            (start, step)
        }
        egui::Direction::BottomUp => {
            let len = rect.width() - socket_padding * 2.0;
            let main_y = if is_input { rect.max.y } else { rect.min.y };
            let start = egui::Pos2::new(rect.min.x + socket_padding, main_y);
            let step = egui::Vec2::new(gap(len), 0.0);
            (start, step)
        }
    };
    for ix in 0..count {
        result.insert(ix, start + step * ix as f32);
    }
    result
}

/// Explicit mode: place sockets at the main-axis edge, using the stored
/// cross-axis positions.
fn resolve_explicit(
    map: &BTreeMap<usize, f32>,
    flow: egui::Direction,
    rect: egui::Rect,
    is_input: bool,
) -> BTreeMap<usize, egui::Pos2> {
    map.iter()
        .map(|(&ix, &cross)| {
            let pos = match flow {
                egui::Direction::LeftToRight => {
                    let main_x = if is_input { rect.min.x } else { rect.max.x };
                    egui::Pos2::new(main_x, cross)
                }
                egui::Direction::RightToLeft => {
                    let main_x = if is_input { rect.max.x } else { rect.min.x };
                    egui::Pos2::new(main_x, cross)
                }
                egui::Direction::TopDown => {
                    let main_y = if is_input { rect.min.y } else { rect.max.y };
                    egui::Pos2::new(cross, main_y)
                }
                egui::Direction::BottomUp => {
                    let main_y = if is_input { rect.max.y } else { rect.min.y };
                    egui::Pos2::new(cross, main_y)
                }
            };
            (ix, pos)
        })
        .collect()
}
