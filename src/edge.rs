use crate::{bezier, EdgesCtx, NodeId};

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
    selected: &'a mut bool,
}

/// A response returned from the [`Edge`] widget.
///
/// Similar to [`egui::Response`], however as there's no clear rectangular space
/// allocated to the edge, we use a more minimal custom response.
pub struct EdgeResponse {
    changed: bool,
    clicked: bool,
    hovered: bool,
    deleted: bool,
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

    /// Process any user interaction with the edge and present it.
    pub fn show(self, ectx: &mut EdgesCtx, ui: &mut egui::Ui) -> EdgeResponse {
        let Self {
            edge: ((a, output), (b, input)),
            distance_per_point,
            selected,
        } = self;

        // Retrieve the location and direction of the node sockets.
        let a_out = ectx.output(ui, a, output).unwrap();
        let b_in = ectx.input(ui, b, input).unwrap();

        // TODO: Cache the curve and its points?
        let bezier = bezier::Cubic::from_edge_points(a_out, b_in);

        // Check the graph `Ui` for interaction.
        let response = ui.response();
        let mouse_pos = response
            .interact_pointer_pos()
            .or(response.hover_pos())
            .unwrap_or_default();

        // If the mouse was clicked on the `Ui`, check for edge interaction.
        let closest_point = bezier.closest_point(distance_per_point, mouse_pos);
        let dist_to_mouse = closest_point.distance(mouse_pos);
        let select_dist = ui.style().interaction.interact_radius;
        let edge_in_progress = ectx.in_progress(ui).is_some();
        let hovered = dist_to_mouse < select_dist
            && !edge_in_progress
            && ectx.closest_socket.is_none()
            && ui.input(|i| !i.pointer.primary_down() || i.pointer.could_any_button_be_click());
        let clicked = hovered && response.clicked();
        let old_selected = *selected;
        let under_selection_rect = ectx
            .selection_rect
            .map(|rect| bezier.intersects_rect(distance_per_point, rect))
            .unwrap_or(false);

        // If already selected and clicked with ctrl down, or a press happened
        // elsewhere and ctrl was *not* held, deselect.
        if *selected {
            if edge_in_progress
                || (clicked && ui.input(|i| i.modifiers.ctrl))
                || ui.input(|i| i.pointer.primary_pressed() && !i.modifiers.ctrl)
            {
                *selected = false;
            }
        // Otherwise if clicked, select.
        } else if clicked {
            *selected = true;
        // Otherwise, check if a selection rect would indicate selection.
        } else if under_selection_rect
            && ui.input(|i| i.modifiers.shift && i.pointer.primary_released())
        {
            *selected = true;
        }

        // Check if the edge was deleted.
        let mut deleted = false;
        // FIXME: We may only want to do this if `ui.id()` has focus
        // (Memory::has_focus) or similar, but we still need to setup proper
        // focus-requesting and consider how to handle nodes too.
        if *selected && !ui.ctx().wants_keyboard_input() {
            let del_keys = [egui::Key::Delete, egui::Key::Backspace];
            if ui.input(|i| del_keys.iter().any(|&k| i.key_pressed(k))) {
                deleted = true;
            }
        }

        // Construct the response.
        let changed = old_selected != *selected;
        let response = EdgeResponse {
            changed,
            clicked,
            hovered,
            deleted,
        };

        // Paint the edge based on the interaction.
        let pts: Vec<_> = bezier.flatten(distance_per_point).collect();
        let stroke = if *selected {
            ui.style().visuals.selection.stroke
        } else if hovered {
            ui.style().visuals.widgets.hovered.fg_stroke
        } else if under_selection_rect && ui.input(|i| i.modifiers.shift) {
            ui.style().visuals.widgets.hovered.fg_stroke
        } else {
            ui.style().visuals.widgets.noninteractive.fg_stroke
        };
        ui.painter().add(egui::Shape::line(pts, stroke));

        // Return the response.
        response
    }
}

impl EdgeResponse {
    /// Whether or not the edge selected state changed.
    pub fn changed(&self) -> bool {
        self.changed
    }

    /// The edge was clicked.
    pub fn clicked(&self) -> bool {
        self.clicked
    }

    /// The pointer hovers over the edge.
    pub fn hovered(&self) -> bool {
        self.hovered
    }

    /// The edge was selected while `Delete` or `Backspace` were pressed.
    pub fn deleted(&self) -> bool {
        self.deleted
    }
}
