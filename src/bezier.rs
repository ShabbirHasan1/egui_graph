/// A very basic cubic bezier type for presenting edges between nodes.
#[derive(Clone, Copy, Debug)]
pub struct Cubic {
    pub from: egui::Pos2,
    pub ctrl1: egui::Pos2,
    pub ctrl2: egui::Pos2,
    pub to: egui::Pos2,
}

impl Cubic {
    /// Maximum proportion of the socket-to-socket distance used for control points.
    pub(crate) const MAX_CURVATURE_FACTOR: f32 = 0.5;
    /// Default normalized curvature value.
    pub const DEFAULT_CURVATURE: f32 = 0.5;

    /// Construct a cubic curve from the start and end points (and normals) of an edge.
    ///
    /// The normals of the associated input/output are required in order to determine ctrl points.
    ///
    /// `curvature` is a normalized value in the range `0.0..=1.0`.
    /// Internally this maps to a control-point distance factor capping the
    /// strongest curve at half of the total socket-to-socket distance.
    pub fn from_edge_points(
        a: (egui::Pos2, egui::Vec2),
        b: (egui::Pos2, egui::Vec2),
        curvature: f32,
    ) -> Self {
        let (from, a_norm) = a;
        let (to, b_norm) = b;
        let distance = from.distance(to);
        let curvature_factor = curvature.clamp(0.0, 1.0) * Self::MAX_CURVATURE_FACTOR;
        let ctrl_distance = distance * curvature_factor;
        let ctrl1 = from + a_norm * ctrl_distance;
        let ctrl2 = to + b_norm * ctrl_distance;
        Self {
            from,
            ctrl1,
            ctrl2,
            to,
        }
    }

    /// Sample the curve at `t`, where `t` is in the range 0..=1.
    pub fn sample(&self, t: f32) -> egui::Pos2 {
        let t2 = t * t;
        let t3 = t2 * t;
        let one_t = 1.0 - t;
        let one_t2 = one_t * one_t;
        let one_t3 = one_t2 * one_t;
        let v = self.from.to_vec2() * one_t3
            + self.ctrl1.to_vec2() * 3.0 * one_t2 * t
            + self.ctrl2.to_vec2() * 3.0 * one_t * t2
            + self.to.to_vec2() * t3;
        egui::Pos2::new(v.x, v.y)
    }

    /// Flatten the curve into a list of points, ready to draw a polyline.
    ///
    /// Determines the number of points by first calculating the total distance of the path *from
    /// -> ctrl1 -> ctrl2 -> to* and then dividing that distance by the given distance per point.
    ///
    /// **NOTE**: This should probably use a `tolerance`, however this involves calculating
    /// derivatives and is significantly more complicated than just estimating a number of points
    /// based on the distance between each of the points in the curve. This is imperfect, but
    /// hopefully does the job for drawing edges.
    pub fn flatten(self, distance_per_point: f32) -> impl Iterator<Item = egui::Pos2> {
        let distance = self.from.distance(self.ctrl1)
            + self.ctrl1.distance(self.ctrl2)
            + self.ctrl2.distance(self.to);
        let samples = (distance / distance_per_point).round() as usize;
        (0..=samples).map(move |ix| {
            let t = ix as f32 / samples as f32;
            self.sample(t)
        })
    }

    /// Find the approximate distance of the given point `p` from the curve.
    ///
    /// **NOTE**: Currently, this does a brute-force search along a `flatten`ned curve. Eventually
    /// we should implement a more efficient approach, however this should be plenty efficient for
    /// simple edge selection (the main use-case for this method).
    pub fn closest_point(self, distance_per_point: f32, target: egui::Pos2) -> egui::Pos2 {
        self.flatten(distance_per_point)
            .fold((self.from, f32::MAX), |closest, p| {
                let dist_sq = p.distance_sq(target);
                if dist_sq < closest.1 {
                    (p, dist_sq)
                } else {
                    closest
                }
            })
            .0
    }

    /// Short-hand for producing the maximum possible bounds for the curve without performing any
    /// interpolation.
    fn max_bounds(&self) -> egui::Rect {
        let mut r = egui::Rect::from_min_max(self.from, self.to);
        r.extend_with(self.ctrl1);
        r.extend_with(self.ctrl2);
        r
    }

    /// Whether or not the curve intersects the given line..
    ///
    /// **NOTE**: Currently, this does a brute-force search along a `flatten`ned curve. Eventually
    /// we should implement a more efficient approach, however this should be plenty efficient for
    /// simple edge selection (the main use-case for this method).
    pub fn intersects_line(self, distance_per_point: f32, line: (egui::Pos2, egui::Pos2)) -> bool {
        if !rect_intersects_line(self.max_bounds(), line) {
            return false;
        }
        let mut pts = self.flatten(distance_per_point).peekable();
        while let Some(b1) = pts.next() {
            if let Some(&b2) = pts.peek() {
                if lines_intersect(line, (b1, b2)) {
                    return true;
                }
            }
        }
        false
    }

    /// Whether or not the curve intersects the given rectangle.
    ///
    /// **NOTE**: Currently, this does a brute-force search along a `flatten`ned curve. Eventually
    /// we should implement a more efficient approach, however this should be plenty efficient for
    /// simple edge selection (the main use-case for this method).
    pub fn intersects_rect(self, distance_per_point: f32, rect: egui::Rect) -> bool {
        if !rect.intersects(self.max_bounds()) {
            return false;
        } else if rect.contains(self.from) || rect.contains(self.to) {
            return true;
        }
        let lt = rect.left_top();
        let lb = rect.left_bottom();
        let rt = rect.right_top();
        let rb = rect.right_bottom();
        let lines = [(lt, rt), (rt, rb), (rb, lb)];
        lines
            .iter()
            .any(|&l| self.intersects_line(distance_per_point, l))
    }
}

// True if any of the area of the rect intersects the line.
fn rect_intersects_line(r: egui::Rect, (a, b): (egui::Pos2, egui::Pos2)) -> bool {
    if !r.intersects(egui::Rect::from_two_pos(a, b)) {
        return false;
    } else if r.contains(a) || r.contains(b) {
        return true;
    }
    let lt = r.left_top();
    let lb = r.left_bottom();
    let rt = r.right_top();
    let rb = r.right_bottom();
    lines_intersect((lt, rt), (a, b))
        || lines_intersect((rt, rb), (a, b))
        || lines_intersect((rb, lb), (a, b))
}

// Whether or not the given lines intersect.
fn lines_intersect(a: (egui::Pos2, egui::Pos2), b: (egui::Pos2, egui::Pos2)) -> bool {
    let (a1, a2) = a;
    let (b1, b2) = b;
    fn tri_area(a: egui::Pos2, b: egui::Pos2, c: egui::Pos2) -> f32 {
        (b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y)
    }
    let t1 = tri_area(a1, a2, b1);
    let t2 = tri_area(a1, a2, b2);
    let res1 = (t1 > 0.0) != (t2 > 0.0) && !(t1 == 0.0 && t2 == 0.0);
    let t1 = tri_area(b1, b2, a1);
    let t2 = tri_area(b1, b2, a2);
    let res2 = (t1 > 0.0) != (t2 > 0.0) && !(t1 == 0.0 && t2 == 0.0);
    res1 && res2
}
