//! The unit dependency-graph visualization: a pure [`layout`] pass behind the
//! [`layout::GraphLayout`] trait, plus an SVG [`view::UnitGraph`] component.

pub mod layout;
pub mod view;
