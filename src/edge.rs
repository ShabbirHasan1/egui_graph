use crate::{bezier, EdgesCtx, NodeId};
use std::ops;

/// A simple bezier-curve Edge widget.
///
/// Handles interaction (selection, deselection, deletion) and painting the
/// bezier curve.
///
/// Adopts styling from the following:
///
/// - Selected: `ui.visuals().selection.fg_stroke`.
/// - Hovered: `ui.visuals().widgets.hovered.fg_stroke`.
/// - Otherwise: `ui.visuals().widgets.noninteractive.fg_stroke`.
pub struct Edge<'a> {
    edge: ((NodeId, OutputIx), (NodeId, InputIx)),
    distance_per_point: f32,
    curvature: f32,
    selected: &'a mut bool,
}

/// A response returned from the [`Edge`] widget.
///
/// Similar to [`egui::Response`], however as there's no clear rectangular space
/// allocated to the edge, we use a more minimal custom response.
pub struct EdgeResponse {
    response: egui::Response,
    changed: bool,
    deleted: bool,
    closest_point: egui::Pos2,
}

/// An index of a node's input or output socket.
pub type SocketIx = usize;
/// An index of a node's input socket.
pub type InputIx = SocketIx;
/// An index of a node's output socket.
pub type OutputIx = SocketIx;

impl<'a> Edge<'a> {
    pub const DEFAULT_DISTANCE_PER_POINT: f32 = 5.0;

    /// An edge from node `a`'s output socket to node `b`'s input socket.
    pub fn new(a: (NodeId, OutputIx), b: (NodeId, InputIx), selected: &'a mut bool) -> Self {
        Self {
            edge: (a, b),
            distance_per_point: Self::DEFAULT_DISTANCE_PER_POINT,
            curvature: bezier::Cubic::DEFAULT_CURVATURE,
            selected,
        }
    }

    /// The distance-per-point used to render the bezier curve path.
    ///
    /// This path is also used to check for selection interaction.
    ///
    /// The smaller the distance, the higher-quality rendering and interactions
    /// will be, at the cost of performance.
    ///
    /// Default: `Self::DEFAULT_DISTANCE_PER_POINT`
    pub fn distance_per_point(mut self, dist: f32) -> Self {
        self.distance_per_point = dist;
        self
    }

    /// Set the normalized curvature used when constructing the edge bezier.
    ///
    /// Values are clamped to `0.0..=1.0` and then scaled internally so the
    /// strongest curve uses at most half the socket-to-socket distance for its
    /// control points.
    ///
    /// Default: [`bezier::Cubic::DEFAULT_CURVATURE`].
    pub fn curvature_factor(mut self, curvature: f32) -> Self {
        self.curvature = curvature;
        self
    }

    /// Process any user interaction with the edge and present it.
    pub fn show(self, ectx: &mut EdgesCtx, ui: &mut egui::Ui) -> EdgeResponse {
        let Self {
            edge: ((a, output), (b, input)),
            distance_per_point,
            curvature,
            selected,
        } = self;

        // Retrieve the location and direction of the node sockets.
        // If either socket position is unavailable (e.g. sparse explicit
        // layout), skip rendering entirely.
        let (a_out, b_in) = match (ectx.output(ui, a, output), ectx.input(ui, b, input)) {
            (Some(a_out), Some(b_in)) => (a_out, b_in),
            _ => {
                let edge_id = ui.id().with(("edge", a, output, b, input));
                let response = ui.interact(egui::Rect::NOTHING, edge_id, egui::Sense::click());
                return EdgeResponse {
                    response,
                    changed: false,
                    deleted: false,
                    closest_point: egui::Pos2::ZERO,
                };
            }
        };

        // TODO: Cache the curve and its points?
        let bezier = bezier::Cubic::from_edge_points(a_out, b_in, curvature);

        // Get the mouse position for computing the closest point on the edge.
        let ui_response = ui.response();
        let mouse_pos = ui_response
            .interact_pointer_pos()
            .or(ui_response.hover_pos())
            .unwrap_or_default();
        let closest_point = bezier.closest_point(distance_per_point, mouse_pos);

        // Create a per-edge response for interaction and context menu support.
        // The interact area follows the mouse along the edge curve.
        let select_dist = ui.style().interaction.interact_radius;
        let edge_id = ui.id().with(("edge", a, output, b, input));
        let interact_rect = egui::Rect::from_center_size(
            closest_point,
            egui::vec2(select_dist * 2.0, select_dist * 2.0),
        );
        let response = ui.interact(interact_rect, edge_id, egui::Sense::click());

        // Determine if edge interactions should be processed.
        // Disable when drawing a new edge or when close to a socket.
        let edge_in_progress = ectx.in_progress(ui).is_some();
        let can_interact = !edge_in_progress && ectx.closest_socket.is_none();
        let clicked = can_interact && response.clicked();

        // Check if the edge intersects the selection rectangle.
        let under_selection_rect = ectx
            .selection_rect
            .map(|rect| bezier.intersects_rect(distance_per_point, rect))
            .unwrap_or(false);

        // Handle selection state changes.
        let old_selected = *selected;
        if *selected {
            // Deselect if: edge drawing started, ctrl+click, or click elsewhere without ctrl.
            if edge_in_progress
                || (clicked && ui.input(|i| i.modifiers.ctrl))
                || ui.input(|i| i.pointer.primary_pressed() && !i.modifiers.ctrl)
            {
                *selected = false;
            }
        } else if clicked
            || (under_selection_rect
                && ui.input(|i| i.modifiers.shift && i.pointer.primary_released()))
        {
            *selected = true;
        }

        // Check if the edge was deleted (skip when immutable).
        let mut deleted = false;
        // FIXME: We may only want to do this if `ui.id()` has focus
        // (Memory::has_focus) or similar, but we still need to setup proper
        // focus-requesting and consider how to handle nodes too.
        if !ectx.immutable && *selected && !ui.ctx().egui_wants_keyboard_input() {
            let del_keys = [egui::Key::Delete, egui::Key::Backspace];
            if ui.input(|i| del_keys.iter().any(|&k| i.key_pressed(k))) {
                deleted = true;
            }
        }

        // Determine hover styling (additional conditions beyond response.hovered()).
        let show_hover = can_interact
            && response.hovered()
            && ui.input(|i| !i.pointer.primary_down() || i.pointer.could_any_button_be_click());

        // Paint the edge.
        let pts: Vec<_> = bezier.flatten(distance_per_point).collect();
        let stroke = if *selected {
            ui.style().visuals.selection.stroke
        } else if show_hover || (under_selection_rect && ui.input(|i| i.modifiers.shift)) {
            ui.style().visuals.widgets.hovered.fg_stroke
        } else {
            ui.style().visuals.widgets.noninteractive.fg_stroke
        };
        ui.painter().add(egui::Shape::line(pts, stroke));

        // Construct and return the response.
        let changed = old_selected != *selected;
        EdgeResponse {
            response,
            changed,
            deleted,
            closest_point,
        }
    }
}

impl EdgeResponse {
    /// Whether or not the edge selected state changed.
    pub fn changed(&self) -> bool {
        self.changed
    }

    /// The edge was selected while `Delete` or `Backspace` were pressed.
    pub fn deleted(&self) -> bool {
        self.deleted
    }

    /// The position on the edge closest to the pointer.
    pub fn closest_point(&self) -> egui::Pos2 {
        self.closest_point
    }
}

impl ops::Deref for EdgeResponse {
    type Target = egui::Response;
    fn deref(&self) -> &Self::Target {
        &self.response
    }
}

impl From<EdgeResponse> for egui::Response {
    fn from(response: EdgeResponse) -> Self {
        response.response
    }
}
