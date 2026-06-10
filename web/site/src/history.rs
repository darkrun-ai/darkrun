//! Client-side history of browsed workspaces.
//!
//! Like the predecessor, `/browse` remembers the repositories you've opened —
//! stored in `localStorage`, never on a server — so the landing page is a short
//! list of where you've been, not a wall of reference. Most-recent first,
//! de-duplicated, capped.

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

use crate::remote::RepoRef;

/// The localStorage key the browse history lives under.
const KEY: &str = "darkrun.browse.history";

/// The most repositories to remember.
const CAP: usize = 12;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    host: String,
    owner: String,
    repo: String,
}

/// Record a visit to `repo` — move it to the front, de-duplicate, cap the list.
/// Fire-and-forget; the read-modify-write happens in one storage snippet.
pub fn record(repo: &RepoRef) {
    let host = serde_json::to_string(&repo.host).unwrap_or_default();
    let owner = serde_json::to_string(&repo.owner).unwrap_or_default();
    let name = serde_json::to_string(&repo.repo).unwrap_or_default();
    let _ = document::eval(&format!(
        "try{{\
           var k='{KEY}';\
           var list=JSON.parse(localStorage.getItem(k)||'[]');\
           list=list.filter(function(e){{return !(e.host==={host}&&e.owner==={owner}&&e.repo==={name});}});\
           list.unshift({{host:{host},owner:{owner},repo:{name}}});\
           if(list.length>{CAP})list=list.slice(0,{CAP});\
           localStorage.setItem(k,JSON.stringify(list));\
         }}catch(e){{}}"
    ));
}

/// The remembered repositories, most-recent first.
pub async fn recent() -> Vec<RepoRef> {
    let raw = document::eval(&format!("return (localStorage.getItem('{KEY}')||'[]');"))
        .join::<String>()
        .await
        .unwrap_or_else(|_| "[]".to_string());
    serde_json::from_str::<Vec<Entry>>(&raw)
        .unwrap_or_default()
        .into_iter()
        .filter(|e| !e.host.is_empty() && !e.owner.is_empty() && !e.repo.is_empty())
        .map(|e| RepoRef { host: e.host, owner: e.owner, repo: e.repo })
        .collect()
}

/// Forget a single remembered repository.
pub fn forget(repo: &RepoRef) {
    let host = serde_json::to_string(&repo.host).unwrap_or_default();
    let owner = serde_json::to_string(&repo.owner).unwrap_or_default();
    let name = serde_json::to_string(&repo.repo).unwrap_or_default();
    let _ = document::eval(&format!(
        "try{{\
           var k='{KEY}';\
           var list=JSON.parse(localStorage.getItem(k)||'[]');\
           list=list.filter(function(e){{return !(e.host==={host}&&e.owner==={owner}&&e.repo==={name});}});\
           localStorage.setItem(k,JSON.stringify(list));\
         }}catch(e){{}}"
    ));
}
