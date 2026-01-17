use eframe::egui;
use egui_graph::node::{EdgeEvent, SocketKind};
use petgraph::graph::{EdgeIndex, NodeIndex};
use petgraph::visit::EdgeRef;
use std::collections::{HashMap, HashSet};

fn main() -> Result<(), eframe::Error> {
    env_logger::init(); // Log to stderr (if you run with `RUST_LOG=debug`).
    let options = eframe::NativeOptions::default();
    let name = "`egui_graph` demo";
    eframe::run_native(name, options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}

struct App {
    state: State,
    view: egui_graph::View,
}

struct State {
    graph: Graph,
    interaction: Interaction,
    flow: egui::Direction,
    socket_radius: f32,
    socket_color: egui::Color32,
    custom_edge_style: bool,
    edge_width: f32,
    edge_color: egui::Color32,
    #[cfg(feature = "layout")]
    auto_layout: bool,
    node_spacing: [f32; 2],
    node_id_map: HashMap<egui_graph::NodeId, NodeIndex>,
    center_view: bool,
    dot_grid: bool,
}

#[derive(Default)]
struct Interaction {
    selection: Selection,
    edge_in_progress: Option<(NodeIndex, SocketKind, usize)>,
}

#[derive(Default)]
struct Selection {
    nodes: HashSet<NodeIndex>,
    edges: HashSet<EdgeIndex>,
}

type Graph = petgraph::stable_graph::StableGraph<Node, (usize, usize)>;

struct Node {
    name: String,
    kind: NodeKind,
}

enum NodeKind {
    Label,
    Button,
    Slider(f32),
    DragValue(f32),
    Comment(String),
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let ctx = &cc.egui_ctx;
        ctx.set_fonts(egui::FontDefinitions::default());
        let graph = new_graph();
        let state = State {
            graph,
            interaction: Default::default(),
            socket_color: ctx.style().visuals.weak_text_color(),
            socket_radius: 3.0,
            custom_edge_style: false,
            edge_width: 1.0,
            edge_color: ctx.style().visuals.weak_text_color(),
            flow: egui::Direction::TopDown,
            #[cfg(feature = "layout")]
            auto_layout: true,
            node_spacing: [1.0, 1.0],
            node_id_map: Default::default(),
            center_view: false,
            dot_grid: true,
        };
        let view = Default::default();
        App { view, state }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(feature = "layout")]
        if self.state.auto_layout {
            self.view.layout = layout(
                &self.state.graph,
                self.state.flow,
                self.state.node_spacing,
                ctx,
            );
        }
        gui(ctx, &mut self.view, &mut self.state);
    }
}

fn new_graph() -> Graph {
    // The graph we want to inspect/edit.
    let mut graph = Graph::new();
    let a = graph.add_node(node("Foo", NodeKind::Label));
    let b = graph.add_node(node("Bar", NodeKind::Button));
    let c = graph.add_node(node("Baz", NodeKind::Slider(0.5)));
    let d = graph.add_node(node("Qux", NodeKind::DragValue(20.0)));
    let comment = "Nodes are a thin wrapper around the `egui::Window`, \
        allowing you to set arbitrary widgets.";
    let e = graph.add_node(node("Fiz", NodeKind::Comment(comment.to_string())));
    graph.add_edge(a, c, (0, 0));
    graph.add_edge(a, d, (1, 1));
    graph.add_edge(b, d, (0, 2));
    graph.add_edge(c, d, (0, 0));
    graph.add_edge(d, e, (0, 0));
    graph
}

fn node(name: impl ToString, kind: NodeKind) -> Node {
    let name = name.to_string();
    Node { name, kind }
}

/// Get the egui::Id for the demo graph widget.
fn graph_id() -> egui::Id {
    egui_graph::id("Demo Graph")
}

#[cfg(feature = "layout")]
fn layout(
    graph: &Graph,
    flow: egui::Direction,
    node_spacing: [f32; 2],
    ctx: &egui::Context,
) -> egui_graph::Layout {
    let graph_id = graph_id();
    ctx.memory(|m| {
        let nodes = graph.node_indices().map(|n| {
            let node_id = egui_graph::NodeId::from_u64(n.index() as u64);
            // Use the helper to get the egui::Id for area_rect lookup
            let egui_id = egui_graph::node::egui_id(graph_id, node_id);
            let size = m
                .area_rect(egui_id)
                .map(|a| a.size())
                .unwrap_or([200.0, 50.0].into());
            (node_id, size)
        });
        let edges = graph
            .edge_indices()
            .filter_map(|e| graph.edge_endpoints(e))
            .map(|(a, b)| {
                (
                    egui_graph::NodeId::from_u64(a.index() as u64),
                    egui_graph::NodeId::from_u64(b.index() as u64),
                )
            });
        let mut layout = egui_graph::layout(nodes, edges, flow);
        // Apply custom offset spacing to the layout
        for pos in layout.values_mut() {
            pos.x *= node_spacing[0];
            pos.y *= node_spacing[1];
        }
        layout
    })
}

fn gui(ctx: &egui::Context, view: &mut egui_graph::View, state: &mut State) {
    egui::containers::CentralPanel::default()
        .frame(egui::Frame::default())
        .show(ctx, |ui| {
            graph_config(ui, view, state);
            graph(ui, view, state);
        });
}

fn graph(ui: &mut egui::Ui, view: &mut egui_graph::View, state: &mut State) {
    egui_graph::Graph::from_id(graph_id())
        .center_view(state.center_view)
        .dot_grid(state.dot_grid)
        .show(view, ui, |ui, show| {
            show.nodes(ui, |nctx, ui| nodes(nctx, ui, state))
                .edges(ui, |ectx, ui| {
                    if state.custom_edge_style {
                        set_edge_style(ui.style_mut(), state);
                    }
                    edges(ectx, ui, state)
                });
        });
}

fn set_edge_style(style: &mut egui::Style, state: &mut State) {
    let vis_mut = &mut style.visuals;
    // Edges use `noninteractive` by default, `hovered` when hovered.
    vis_mut.widgets.noninteractive.fg_stroke.color = state.edge_color;
    vis_mut.widgets.noninteractive.fg_stroke.width = state.edge_width;
    vis_mut.widgets.hovered.fg_stroke.color = state.edge_color.linear_multiply(1.25);
    vis_mut.widgets.hovered.fg_stroke.width = state.edge_width;
    // Exaggerate the color and width when selected.
    vis_mut.selection.stroke.color = state.edge_color.linear_multiply(1.5);
    vis_mut.selection.stroke.width = state.edge_width * 3.0;
}

fn nodes(nctx: &mut egui_graph::NodesCtx, ui: &mut egui::Ui, state: &mut State) {
    let indices: Vec<_> = state.graph.node_indices().collect();
    for n in indices {
        let inputs = state
            .graph
            .edges_directed(n, petgraph::Incoming)
            .fold(0, |max, e| std::cmp::max(max, e.weight().1 + 1));
        let outputs = state
            .graph
            .edges_directed(n, petgraph::Outgoing)
            .fold(0, |max, e| std::cmp::max(max, e.weight().0 + 1));
        let node = &mut state.graph[n];
        let node_id = egui_graph::NodeId::from_u64(n.index() as u64);
        state.node_id_map.insert(node_id, n);
        let response = egui_graph::node::Node::from_id(node_id)
            .inputs(inputs)
            .outputs(outputs)
            .flow(state.flow)
            .socket_radius(state.socket_radius)
            .socket_color(state.socket_color)
            .show(nctx, ui, |ui| match node.kind {
                NodeKind::Label => {
                    ui.label(&node.name);
                }
                NodeKind::Button => {
                    ui.horizontal(|ui| {
                        if ui.button(&node.name).clicked() {
                            println!("{}", node.name);
                        }
                    });
                }
                NodeKind::DragValue(ref mut f) => {
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(f).range(0.0..=255.0));
                    });
                }
                NodeKind::Slider(ref mut f) => {
                    ui.horizontal(|ui| ui.add(egui::Slider::new(f, 0.0..=1.0)));
                }
                NodeKind::Comment(ref mut text) => {
                    ui.text_edit_multiline(text);
                }
            });

        if response.changed() {
            // Update the selected nodes.
            if egui_graph::is_node_selected(ui, nctx.graph_id, node_id) {
                state.interaction.selection.nodes.insert(n);
            } else {
                state.interaction.selection.nodes.remove(&n);
            }

            // Check for an edge event.
            if let Some(ev) = response.edge_event() {
                match ev {
                    EdgeEvent::Started { kind, index } => {
                        state.interaction.edge_in_progress = Some((n, kind, index));
                    }
                    EdgeEvent::Ended { kind, index } => {
                        // Create the edge.
                        if let Some((src, _, ix)) = state.interaction.edge_in_progress.take() {
                            let (a, b, w) = match kind {
                                SocketKind::Input => (src, n, (ix, index)),
                                SocketKind::Output => (n, src, (index, ix)),
                            };
                            // Check that this edge doesn't already exist.
                            if !state
                                .graph
                                .edges(a)
                                .any(|e| e.target() == b && *e.weight() == w)
                            {
                                state.graph.add_edge(a, b, w);
                            }
                        }
                    }
                    EdgeEvent::Cancelled => {
                        state.interaction.edge_in_progress = None;
                    }
                }
            }

            // If the delete key was pressed while selected, remove it.
            if response.removed() {
                state.graph.remove_node(n);
                state.node_id_map.remove(&node_id);
            }
        }
    }
}

fn edges(ectx: &mut egui_graph::EdgesCtx, ui: &mut egui::Ui, state: &mut State) {
    // Instantiate all edges.
    for e in state.graph.edge_indices().collect::<Vec<_>>() {
        let (na, nb) = state.graph.edge_endpoints(e).unwrap();
        let (output, input) = *state.graph.edge_weight(e).unwrap();
        let a = egui_graph::NodeId::from_u64(na.index() as u64);
        let b = egui_graph::NodeId::from_u64(nb.index() as u64);
        let mut selected = state.interaction.selection.edges.contains(&e);
        let response =
            egui_graph::edge::Edge::new((a, output), (b, input), &mut selected).show(ectx, ui);

        if response.deleted() {
            state.graph.remove_edge(e);
            state.interaction.selection.edges.remove(&e);
        } else if response.changed() {
            if selected {
                state.interaction.selection.edges.insert(e);
            } else {
                state.interaction.selection.edges.remove(&e);
            }
        }
    }

    // Draw the in-progress edge if there is one.
    if let Some(edge) = ectx.in_progress(ui) {
        edge.show(ui);
    }
}

fn graph_config(ui: &mut egui::Ui, view: &mut egui_graph::View, state: &mut State) {
    let mut frame = egui::Frame::window(ui.style());
    frame.shadow.spread = 0;
    frame.shadow.offset = [0, 0];
    egui::Window::new("Graph Config")
        .frame(frame)
        .anchor(
            egui::Align2::LEFT_TOP,
            ui.spacing().window_margin.left_top(),
        )
        .collapsible(false)
        .title_bar(false)
        .auto_sized()
        .show(ui.ctx(), |ui| {
            ui.label("GRAPH CONFIG");
            #[cfg(feature = "layout")]
            ui.horizontal(|ui| {
                ui.checkbox(&mut state.auto_layout, "Automatic Layout");
                ui.separator();
                ui.add_enabled_ui(!state.auto_layout, |ui| {
                    if ui.button("Layout Once").clicked() {
                        view.layout =
                            layout(&state.graph, state.flow, state.node_spacing, ui.ctx());
                    }
                });
            });
            ui.checkbox(&mut state.dot_grid, "Show Dot Grid");
            ui.checkbox(&mut state.center_view, "Center View");
            ui.horizontal(|ui| {
                ui.label("Flow:");
                ui.radio_value(&mut state.flow, egui::Direction::LeftToRight, "Right");
                ui.radio_value(&mut state.flow, egui::Direction::TopDown, "Down");
            });
            ui.horizontal(|ui| {
                ui.label("Node spacing X:");
                ui.add(egui::Slider::new(&mut state.node_spacing[0], 0.75..=2.0));
            });
            ui.horizontal(|ui| {
                ui.label("Node spacing Y:");
                ui.add(egui::Slider::new(&mut state.node_spacing[1], 0.5..=2.0));
            });
            ui.checkbox(&mut state.custom_edge_style, "Custom Edge Style");
            ui.add_enabled_ui(state.custom_edge_style, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Edge width:");
                    ui.add(egui::Slider::new(&mut state.edge_width, 0.5..=10.0));
                });
                ui.horizontal(|ui| {
                    ui.label("Socket radius:");
                    ui.add(egui::Slider::new(&mut state.socket_radius, 1.0..=10.0));
                });
                ui.horizontal(|ui| {
                    ui.label("Edge color:");
                    ui.color_edit_button_srgba(&mut state.edge_color);
                    ui.label("Socket color:");
                    ui.color_edit_button_srgba(&mut state.socket_color);
                });
            });
            ui.label(format!("Scene: {:?}", view.scene_rect));
        });
}
