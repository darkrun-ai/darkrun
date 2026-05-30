//! darkrun-ui — darkrun's shared design system.
//!
//! One crate, two consumers: the Dioxus desktop app and the website. It carries
//! the **dark-only** design tokens (near-black surfaces, a single cool-cyan
//! accent, the six phase hues) and a set of Dioxus components built on top of
//! them. The crate is renderer-agnostic: it depends on Dioxus' macro/html/hooks
//! surface only, so it compiles for both native and `wasm32-unknown-unknown`.
//!
//! ## Layout
//!
//! - [`tokens`] — color/type/spacing constants plus the [`tokens::THEME_CSS`]
//!   custom-property block. Source of truth for both Rust styling and CSS.
//! - [`kinds`] — small `Copy` enums ([`kinds::Phase`], [`kinds::Tone`],
//!   [`kinds::Step`]) shared across components. No `darkrun-core` dependency.
//! - [`components`] — [`Wordmark`], [`Card`], [`Badge`], [`Button`],
//!   [`StationPipeline`], [`FactoryCard`], [`UnitRow`], [`CheckpointBar`].
//! - [`graph`] — the SVG unit-DAG visualization with a pluggable
//!   [`graph::layout::GraphLayout`] (default layered/Sugiyama-ish placement).
//!
//! ## Usage
//!
//! ```ignore
//! use darkrun_ui::prelude::*;
//!
//! fn app() -> Element {
//!     rsx! {
//!         style { "{darkrun_ui::tokens::THEME_CSS}" }
//!         Wordmark { variant: WordmarkVariant::Filled, size: 32.0 }
//!         FactoryCard {
//!             title: "Ship the importer".to_string(),
//!             factory: "software-factory".to_string(),
//!             station: Some("build".to_string()),
//!             phase: Some(Phase::Manufacture),
//!         }
//!     }
//! }
//! ```

pub mod components;
pub mod graph;
pub mod kinds;
pub mod tokens;

/// The recommended glob import for consumers: every public component, the shared
/// kinds, and the graph types, plus Dioxus' own prelude.
pub mod prelude {
    pub use dioxus::prelude::*;

    pub use crate::components::factory::{
        CheckpointBar, CheckpointKind, FactoryCard, UnitRow,
    };
    pub use crate::components::pipeline::{strip_for, PhaseDot, StationPipeline};
    pub use crate::components::primitives::{Badge, Button, ButtonVariant, Card};
    pub use crate::components::wordmark::{Wordmark, WordmarkVariant};
    pub use crate::graph::layout::{
        GraphEdge, GraphLayout, GraphNode, LayeredLayout, LayoutOptions,
        LayoutResult, PlacedEdge, PlacedNode,
    };
    pub use crate::graph::view::{UnitGraph, UnitGraphNode};
    pub use crate::kinds::{Phase, Step, Tone};
    pub use crate::tokens;
}
