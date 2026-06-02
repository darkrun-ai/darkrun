//! Pure selection + annotation math behind the visual-question, design-direction,
//! and picker views. No Dioxus, no rendering — just the state transitions and the
//! pin-placement geometry the components drive, kept here so they are trivially
//! testable on native and the components stay thin.
//!
//! Three concerns live here:
//! - [`SelectionModel`] — toggling option ids under a single- or multi-select
//!   policy, with a stable, order-preserving selected set.
//! - [`place_pin`] / [`PinPoint`] — converting a click at pixel coordinates over a
//!   preview image into the normalized `0..1` pin coordinate the wire carries
//!   (and back, for rendering).
//! - small helpers ([`is_selected`], [`selected_in_order`]) used by both.

/// The selection policy a question enforces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectMode {
    /// Exactly one option may be chosen; picking a new one replaces the old.
    Single,
    /// Any number of options may be chosen; picking toggles membership.
    Multi,
}

impl SelectMode {
    /// Derive the mode from the wire's `multi_select` flag.
    pub fn from_multi(multi: bool) -> Self {
        if multi {
            SelectMode::Multi
        } else {
            SelectMode::Single
        }
    }

    /// Whether this mode permits more than one concurrent selection.
    pub fn is_multi(self) -> bool {
        matches!(self, SelectMode::Multi)
    }
}

/// An order-preserving set of selected option ids under a [`SelectMode`].
///
/// The selected ids are kept in the order they were first chosen (not the order
/// of the option list), so the submitted answer reflects the operator's intent.
/// Re-selecting an already-selected id in [`SelectMode::Single`] is a no-op;
/// in [`SelectMode::Multi`] it deselects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionModel {
    mode: SelectMode,
    selected: Vec<String>,
}

impl SelectionModel {
    /// An empty selection under `mode`.
    pub fn new(mode: SelectMode) -> Self {
        Self { mode, selected: Vec::new() }
    }

    /// Seed a selection from an existing answer (e.g. a re-opened, already
    /// answered session). Ids are de-duplicated; under [`SelectMode::Single`]
    /// only the first survives.
    pub fn from_selected(mode: SelectMode, ids: impl IntoIterator<Item = String>) -> Self {
        let mut model = Self::new(mode);
        for id in ids {
            // Seeding mirrors a fresh pick: single-select keeps only the last,
            // multi-select accumulates uniquely.
            match mode {
                SelectMode::Single => model.selected = vec![id],
                SelectMode::Multi => {
                    if !model.selected.iter().any(|s| s == &id) {
                        model.selected.push(id);
                    }
                }
            }
        }
        model
    }

    /// The active selection mode.
    pub fn mode(&self) -> SelectMode {
        self.mode
    }

    /// The selected ids, in selection order.
    pub fn selected(&self) -> &[String] {
        &self.selected
    }

    /// Whether `id` is currently selected.
    pub fn is_selected(&self, id: &str) -> bool {
        self.selected.iter().any(|s| s == id)
    }

    /// How many options are currently selected.
    pub fn count(&self) -> usize {
        self.selected.len()
    }

    /// Whether any option is selected — the gate on enabling a submit button.
    pub fn is_empty(&self) -> bool {
        self.selected.is_empty()
    }

    /// Toggle `id` under the active policy, returning the new selection state of
    /// that id (`true` = now selected).
    ///
    /// - Single-select: choosing a new id replaces the current one; choosing the
    ///   already-selected id clears it (so a single-select can be emptied).
    /// - Multi-select: toggles membership, preserving the order of the survivors.
    pub fn toggle(&mut self, id: &str) -> bool {
        let already = self.is_selected(id);
        match self.mode {
            SelectMode::Single => {
                if already {
                    self.selected.clear();
                    false
                } else {
                    self.selected = vec![id.to_string()];
                    true
                }
            }
            SelectMode::Multi => {
                if already {
                    self.selected.retain(|s| s != id);
                    false
                } else {
                    self.selected.push(id.to_string());
                    true
                }
            }
        }
    }

    /// Clear the selection.
    pub fn clear(&mut self) {
        self.selected.clear();
    }
}

/// A normalized pin coordinate in `0..1` over a preview image, paired with its
/// note — exactly the shape the wire carries (`DirectionPin`).
#[derive(Debug, Clone, PartialEq)]
pub struct PinPoint {
    /// X in `0..1`, relative to the preview width.
    pub x: f64,
    /// Y in `0..1`, relative to the preview height.
    pub y: f64,
    /// The note attached to this pin.
    pub note: String,
}

impl PinPoint {
    /// Construct a pin from already-normalized coordinates, clamping into `0..1`.
    pub fn new(x: f64, y: f64, note: impl Into<String>) -> Self {
        Self { x: clamp01(x), y: clamp01(y), note: note.into() }
    }

    /// The pin's left offset as a CSS percentage string (e.g. `"42.5%"`), for
    /// absolute positioning over the preview.
    pub fn left_pct(&self) -> String {
        format!("{:.4}%", self.x * 100.0)
    }

    /// The pin's top offset as a CSS percentage string.
    pub fn top_pct(&self) -> String {
        format!("{:.4}%", self.y * 100.0)
    }
}

/// Convert a click at pixel offset `(px, py)` inside a preview of size
/// `(width, height)` into a normalized [`PinPoint`].
///
/// Coordinates are clamped into the box and divided by the dimensions; a
/// zero-or-negative dimension degrades to `0.0` on that axis rather than
/// producing a NaN/Inf. The note is attached verbatim.
pub fn place_pin(px: f64, py: f64, width: f64, height: f64, note: impl Into<String>) -> PinPoint {
    let x = normalize(px, width);
    let y = normalize(py, height);
    PinPoint::new(x, y, note)
}

/// Normalize a pixel offset along an axis of length `dim` into `0..1`. A
/// non-positive `dim` yields `0.0` (no division by zero / Inf).
fn normalize(offset: f64, dim: f64) -> f64 {
    if dim <= 0.0 || !dim.is_finite() {
        0.0
    } else {
        clamp01(offset / dim)
    }
}

/// Clamp a float into the inclusive `0.0..=1.0` range, mapping NaN to `0.0`.
fn clamp01(v: f64) -> f64 {
    if v.is_nan() {
        0.0
    } else {
        v.clamp(0.0, 1.0)
    }
}

/// The geometry a placed visual mark carries, in normalized `0..1` space.
///
/// One variant per annotate shape: a `pin` is a point, a `box`/`highlight` is a
/// rectangle, an `arrow` is a tail→head segment, and a `pen` stroke is a
/// polyline. The host maps these directly onto the wire's `ImageShape`
/// (`pin`/`rect`/`arrow`/`path`/`highlight`) — this module owns only the math so
/// it stays renderer- and wire-agnostic. The `note` rides along for the comment
/// thread, exactly like [`PinPoint::note`].
#[derive(Debug, Clone, PartialEq)]
pub enum VisualMark {
    /// A single point — the `pin` tool.
    Pin {
        /// The pin point, `0..1`.
        point: PinPoint,
    },
    /// A rectangle region — the `box` tool.
    Rect {
        /// The drawn region, `0..1`.
        rect: NormBox,
    },
    /// A translucent highlight sweep — the `highlight` tool. Geometrically a
    /// rectangle; kept distinct so the host can pick the `highlight` shape.
    Highlight {
        /// The swept region, `0..1`.
        rect: NormBox,
    },
    /// An arrow from a tail to a head — the `arrow` tool.
    Arrow {
        /// The tail (drag start), `0..1`.
        from: PinPoint,
        /// The head (drag end), `0..1`.
        to: PinPoint,
    },
    /// A freehand polyline — the `pen` tool.
    Path {
        /// The captured stroke points, in draw order, each `0..1`.
        points: Vec<PinPoint>,
    },
}

impl VisualMark {
    /// The lowercase shape slug — the same vocabulary the wire's `ImageShape`
    /// uses (`pin`/`rect`/`arrow`/`path`/`highlight`), so the host maps it 1:1.
    pub fn shape_slug(&self) -> &'static str {
        match self {
            VisualMark::Pin { .. } => "pin",
            VisualMark::Rect { .. } => "rect",
            VisualMark::Highlight { .. } => "highlight",
            VisualMark::Arrow { .. } => "arrow",
            VisualMark::Path { .. } => "path",
        }
    }

    /// A single representative point for the mark, used where only one anchor
    /// coordinate is carried (e.g. the legacy pin channel): a pin's point, a
    /// rect/highlight's top-left, an arrow's head, or a path's first point.
    pub fn anchor_point(&self) -> PinPoint {
        match self {
            VisualMark::Pin { point } => point.clone(),
            VisualMark::Rect { rect } | VisualMark::Highlight { rect } => {
                PinPoint::new(rect.x, rect.y, String::new())
            }
            VisualMark::Arrow { to, .. } => to.clone(),
            VisualMark::Path { points } => {
                points.first().cloned().unwrap_or_else(|| PinPoint::new(0.0, 0.0, String::new()))
            }
        }
    }

    /// The note carried with the mark, for the comment thread.
    pub fn note(&self) -> &str {
        match self {
            VisualMark::Pin { point } => &point.note,
            VisualMark::Arrow { to, .. } => &to.note,
            VisualMark::Rect { rect } | VisualMark::Highlight { rect } => &rect.note,
            VisualMark::Path { points } => points.last().map(|p| p.note.as_str()).unwrap_or(""),
        }
    }
}

/// A normalized rectangle in `0..1` space — top-left `(x, y)` plus size
/// `(w, h)`, each a fraction of the stage — with a note. Mirrors the wire's
/// `NormRect`, kept here so the UI math has no `darkrun-api` dependency.
#[derive(Debug, Clone, PartialEq)]
pub struct NormBox {
    /// Left edge, `0..1`.
    pub x: f64,
    /// Top edge, `0..1`.
    pub y: f64,
    /// Width, `0..1`.
    pub w: f64,
    /// Height, `0..1`.
    pub h: f64,
    /// The note attached to this region.
    pub note: String,
}

impl NormBox {
    /// Construct a box, clamping the origin into `0..1` and the size so the box
    /// never runs past the stage edge.
    pub fn new(x: f64, y: f64, w: f64, h: f64, note: impl Into<String>) -> Self {
        let x = clamp01(x);
        let y = clamp01(y);
        Self {
            x,
            y,
            w: clamp01(w).min(1.0 - x),
            h: clamp01(h).min(1.0 - y),
            note: note.into(),
        }
    }

    /// Build a box from two opposite corner points (a drag), normalizing so the
    /// origin is the top-left regardless of drag direction.
    pub fn from_corners(ax: f64, ay: f64, bx: f64, by: f64, note: impl Into<String>) -> Self {
        let x0 = ax.min(bx);
        let y0 = ay.min(by);
        let w = (ax - bx).abs();
        let h = (ay - by).abs();
        Self::new(x0, y0, w, h, note)
    }
}

/// Convert a pixel-space drag rectangle inside a preview of size
/// `(width, height)` into a normalized [`NormBox`]. The two pixel corners may be
/// given in any order. Degenerate dimensions degrade to `0.0` per axis.
pub fn place_box(
    px: f64,
    py: f64,
    qx: f64,
    qy: f64,
    width: f64,
    height: f64,
    note: impl Into<String>,
) -> NormBox {
    NormBox::from_corners(
        normalize(px, width),
        normalize(py, height),
        normalize(qx, width),
        normalize(qy, height),
        note,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- SelectMode --------------------------------------------------------

    #[test]
    fn mode_from_multi_flag() {
        assert_eq!(SelectMode::from_multi(true), SelectMode::Multi);
        assert_eq!(SelectMode::from_multi(false), SelectMode::Single);
        assert!(SelectMode::Multi.is_multi());
        assert!(!SelectMode::Single.is_multi());
    }

    // --- single-select -----------------------------------------------------

    #[test]
    fn single_select_starts_empty() {
        let m = SelectionModel::new(SelectMode::Single);
        assert!(m.is_empty());
        assert_eq!(m.count(), 0);
        assert!(m.selected().is_empty());
    }

    #[test]
    fn single_select_picks_one() {
        let mut m = SelectionModel::new(SelectMode::Single);
        assert!(m.toggle("a"));
        assert!(m.is_selected("a"));
        assert_eq!(m.selected(), ["a".to_string()]);
        assert_eq!(m.count(), 1);
    }

    #[test]
    fn single_select_replaces_prior_choice() {
        let mut m = SelectionModel::new(SelectMode::Single);
        m.toggle("a");
        assert!(m.toggle("b"));
        assert!(!m.is_selected("a"));
        assert!(m.is_selected("b"));
        assert_eq!(m.selected(), ["b".to_string()]);
        assert_eq!(m.count(), 1);
    }

    #[test]
    fn single_select_toggling_same_clears() {
        let mut m = SelectionModel::new(SelectMode::Single);
        m.toggle("a");
        assert!(!m.toggle("a"));
        assert!(m.is_empty());
        assert!(!m.is_selected("a"));
    }

    // --- multi-select ------------------------------------------------------

    #[test]
    fn multi_select_accumulates_in_order() {
        let mut m = SelectionModel::new(SelectMode::Multi);
        m.toggle("a");
        m.toggle("b");
        m.toggle("c");
        assert_eq!(m.selected(), ["a".to_string(), "b".to_string(), "c".to_string()]);
        assert_eq!(m.count(), 3);
    }

    #[test]
    fn multi_select_toggle_off_preserves_remaining_order() {
        let mut m = SelectionModel::new(SelectMode::Multi);
        m.toggle("a");
        m.toggle("b");
        m.toggle("c");
        assert!(!m.toggle("b"));
        assert_eq!(m.selected(), ["a".to_string(), "c".to_string()]);
    }

    #[test]
    fn multi_select_reselect_after_removal_appends_at_end() {
        let mut m = SelectionModel::new(SelectMode::Multi);
        m.toggle("a");
        m.toggle("b");
        m.toggle("a"); // remove a
        m.toggle("a"); // re-add a -> now after b
        assert_eq!(m.selected(), ["b".to_string(), "a".to_string()]);
    }

    #[test]
    fn clear_empties_any_mode() {
        for mode in [SelectMode::Single, SelectMode::Multi] {
            let mut m = SelectionModel::new(mode);
            m.toggle("a");
            m.toggle("b");
            m.clear();
            assert!(m.is_empty());
        }
    }

    // --- seeding -----------------------------------------------------------

    #[test]
    fn from_selected_single_keeps_last() {
        let m = SelectionModel::from_selected(
            SelectMode::Single,
            ["a".to_string(), "b".to_string()],
        );
        assert_eq!(m.selected(), ["b".to_string()]);
    }

    #[test]
    fn from_selected_multi_dedupes_preserving_first_order() {
        let m = SelectionModel::from_selected(
            SelectMode::Multi,
            ["a".to_string(), "b".to_string(), "a".to_string(), "c".to_string()],
        );
        assert_eq!(
            m.selected(),
            ["a".to_string(), "b".to_string(), "c".to_string()]
        );
    }

    #[test]
    fn from_selected_empty_is_empty() {
        let m = SelectionModel::from_selected(SelectMode::Multi, Vec::<String>::new());
        assert!(m.is_empty());
    }

    // --- pin placement -----------------------------------------------------

    #[test]
    fn place_pin_normalizes_center() {
        let p = place_pin(50.0, 25.0, 100.0, 50.0, "middle");
        assert!((p.x - 0.5).abs() < 1e-9);
        assert!((p.y - 0.5).abs() < 1e-9);
        assert_eq!(p.note, "middle");
    }

    #[test]
    fn place_pin_clamps_out_of_bounds() {
        let over = place_pin(150.0, -20.0, 100.0, 50.0, "x");
        assert_eq!(over.x, 1.0);
        assert_eq!(over.y, 0.0);
    }

    #[test]
    fn place_pin_handles_zero_dimension() {
        let p = place_pin(10.0, 10.0, 0.0, 0.0, "edge");
        assert_eq!(p.x, 0.0);
        assert_eq!(p.y, 0.0);
        assert!(p.x.is_finite() && p.y.is_finite());
    }

    #[test]
    fn place_pin_handles_nonfinite_dimension() {
        let p = place_pin(10.0, 10.0, f64::NAN, f64::INFINITY, "weird");
        assert_eq!(p.x, 0.0);
        // infinite dim -> offset/inf would be 0, but we guard non-finite as 0 too
        assert_eq!(p.y, 0.0);
    }

    #[test]
    fn pin_point_clamps_on_construction() {
        let p = PinPoint::new(2.0, -1.0, "n");
        assert_eq!(p.x, 1.0);
        assert_eq!(p.y, 0.0);
    }

    #[test]
    fn pin_point_nan_maps_to_zero() {
        let p = PinPoint::new(f64::NAN, f64::NAN, "n");
        assert_eq!(p.x, 0.0);
        assert_eq!(p.y, 0.0);
    }

    #[test]
    fn pin_point_percentages_render() {
        let p = PinPoint::new(0.425, 0.1, "n");
        assert_eq!(p.left_pct(), "42.5000%");
        assert_eq!(p.top_pct(), "10.0000%");
    }

    #[test]
    fn round_trip_pixel_to_pct_is_consistent() {
        // A pin placed at 1/4 width should render at 25%.
        let p = place_pin(40.0, 0.0, 160.0, 90.0, "q");
        assert_eq!(p.left_pct(), "25.0000%");
    }

    // --- normalized box / drag rectangles ----------------------------------

    #[test]
    fn norm_box_clamps_size_to_the_stage_edge() {
        // A box whose width would run past the right edge is trimmed to fit.
        let b = NormBox::new(0.8, 0.9, 0.5, 0.5, "n");
        assert!((b.w - 0.2).abs() < 1e-9);
        assert!((b.h - 0.1).abs() < 1e-9);
    }

    #[test]
    fn norm_box_from_corners_normalizes_drag_direction() {
        // Dragging bottom-right → top-left yields the same box as the reverse.
        let a = NormBox::from_corners(0.6, 0.7, 0.2, 0.3, "n");
        let b = NormBox::from_corners(0.2, 0.3, 0.6, 0.7, "n");
        assert!((a.x - 0.2).abs() < 1e-9 && (a.y - 0.3).abs() < 1e-9);
        assert!((a.w - 0.4).abs() < 1e-9 && (a.h - 0.4).abs() < 1e-9);
        assert_eq!(a, b);
    }

    #[test]
    fn place_box_normalizes_pixel_corners() {
        // A drag from (40,0) to (120,45) over a 160×90 preview spans 25%..75% x.
        let b = place_box(40.0, 0.0, 120.0, 45.0, 160.0, 90.0, "drag");
        assert!((b.x - 0.25).abs() < 1e-9);
        assert!((b.y - 0.0).abs() < 1e-9);
        assert!((b.w - 0.5).abs() < 1e-9);
        assert!((b.h - 0.5).abs() < 1e-9);
    }

    // --- visual mark shape mapping -----------------------------------------

    #[test]
    fn visual_mark_shape_slugs_match_the_wire_vocabulary() {
        let pin = VisualMark::Pin { point: PinPoint::new(0.5, 0.5, "p") };
        let rect = VisualMark::Rect { rect: NormBox::new(0.1, 0.1, 0.2, 0.2, "r") };
        let hi = VisualMark::Highlight { rect: NormBox::new(0.1, 0.1, 0.2, 0.2, "h") };
        let arrow = VisualMark::Arrow {
            from: PinPoint::new(0.1, 0.1, ""),
            to: PinPoint::new(0.4, 0.4, "a"),
        };
        let path = VisualMark::Path {
            points: vec![PinPoint::new(0.1, 0.1, ""), PinPoint::new(0.2, 0.3, "pen")],
        };
        assert_eq!(pin.shape_slug(), "pin");
        assert_eq!(rect.shape_slug(), "rect");
        assert_eq!(hi.shape_slug(), "highlight");
        assert_eq!(arrow.shape_slug(), "arrow");
        assert_eq!(path.shape_slug(), "path");
    }

    #[test]
    fn visual_mark_anchor_point_picks_the_representative_coordinate() {
        // A rect anchors on its top-left; an arrow on its head; a path on its
        // first point.
        let rect = VisualMark::Rect { rect: NormBox::new(0.3, 0.4, 0.2, 0.2, "r") };
        assert_eq!(rect.anchor_point().x, 0.3);
        assert_eq!(rect.anchor_point().y, 0.4);

        let arrow = VisualMark::Arrow {
            from: PinPoint::new(0.1, 0.1, ""),
            to: PinPoint::new(0.7, 0.6, "a"),
        };
        assert_eq!(arrow.anchor_point().x, 0.7);
        assert_eq!(arrow.anchor_point().y, 0.6);

        let path = VisualMark::Path {
            points: vec![PinPoint::new(0.2, 0.25, ""), PinPoint::new(0.9, 0.9, "")],
        };
        assert_eq!(path.anchor_point().x, 0.2);
        assert_eq!(path.anchor_point().y, 0.25);
    }

    #[test]
    fn visual_mark_carries_its_note() {
        let arrow = VisualMark::Arrow {
            from: PinPoint::new(0.1, 0.1, ""),
            to: PinPoint::new(0.7, 0.6, "point here"),
        };
        assert_eq!(arrow.note(), "point here");
        let path = VisualMark::Path {
            points: vec![PinPoint::new(0.1, 0.1, ""), PinPoint::new(0.2, 0.2, "trace")],
        };
        assert_eq!(path.note(), "trace");
    }
}
