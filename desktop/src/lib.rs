//! Library surface of `darkrun-desktop`.
//!
//! The binary's pure boundary layers — the wire client ([`wire`]) and the
//! domain->UI mapping ([`map`]) — are exposed here so they can be exercised by
//! integration tests (`desktop/tests/*.rs`) without dragging in the Dioxus
//! component tree. The UI layer (`review.rs`) lives only in the binary.

pub mod map;
pub mod wire;
