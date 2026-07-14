//! Engine-free reader for a repo's on-disk `.darkrun/` state.
//!
//! Every function here projects the `.darkrun/` sidecars into a
//! [`darkrun_api`] payload from nothing but a [`darkrun_core::StateStore`] (which
//! is just a repo-root path handle) — no live engine, no HTTP server, no
//! in-memory session registry. This is the shared read path behind two surfaces:
//!
//! - **the HTTP server** ([`darkrun_http`]) — its browse/feedback/proof handlers
//!   are thin adapters that call these and wrap the result in `Json`, and
//! - **the desktop's offline view** — which renders a run straight from disk when
//!   no engine is serving it.
//!
//! Because both read the SAME projection, a run looks identical whether an engine
//! is up or not; the engine is only needed to *advance* the run, never to read
//! it. Writes (operator decisions, annotations) go back through
//! [`darkrun_core::StateStore`] the same way, and a resuming engine re-reads them
//! via its own `derive_position`.

pub mod feedback;
pub mod feedback_doc;
pub mod proof;
pub mod runs;

pub use feedback::{feedback_for_station, wire_reply};
pub use feedback_doc::{next_id, FeedbackDoc};
pub use proof::read_disk_proof;
pub use runs::{list_runs, run_detail};
