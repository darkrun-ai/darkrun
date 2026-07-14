//! Read a run's attached objective-evidence proof off `.darkrun/<run>/proof.json`.

use darkrun_core::StateStore;

/// Read a run's on-disk `proof.json` (written by the engine's proof-attach tool)
/// and return the most relevant proof: the run-level (unscoped) one if present,
/// else the first station-scoped proof. `None` when the file is absent or
/// unreadable. Mirrors the disk shape the engine serializes (a run-level slot
/// plus a station map) without depending on `darkrun-mcp`.
pub fn read_disk_proof(
    store: &StateStore,
    run: &str,
) -> Option<(darkrun_api::proof::Proof, Option<String>)> {
    #[derive(serde::Deserialize)]
    struct DiskProof {
        #[serde(default)]
        run: Option<darkrun_api::proof::Proof>,
        #[serde(default)]
        stations: std::collections::BTreeMap<String, darkrun_api::proof::Proof>,
    }
    let path = store.run_dir(run).join("proof.json");
    let bytes = std::fs::read(path).ok()?;
    let disk: DiskProof = serde_json::from_slice(&bytes).ok()?;
    if let Some(proof) = disk.run {
        return Some((proof, None));
    }
    disk.stations
        .into_iter()
        .next()
        .map(|(station, proof)| (proof, Some(station)))
}
