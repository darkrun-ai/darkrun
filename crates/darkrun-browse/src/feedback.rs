//! Project a run's on-disk feedback sidecars into the [`darkrun_api`] feedback
//! list payload.

use darkrun_api::{AuthorType, FeedbackItem, FeedbackListResponse, FeedbackReply};
use darkrun_core::StateStore;

use crate::feedback_doc::FeedbackDoc;

/// Project one on-disk reply line (`author: text`) onto the wire
/// [`FeedbackReply`]. The sidecar stores replies as flat strings with no
/// per-reply timestamp, so `created_at` is honestly empty rather than invented.
/// A line with no `author:` prefix is attributed to `user` (the flat format's
/// only untagged writer).
pub fn wire_reply(line: &str) -> FeedbackReply {
    let (author, body) = match line.split_once(':') {
        Some((a, b)) if !a.trim().is_empty() => (a.trim().to_string(), b.trim().to_string()),
        _ => ("user".to_string(), line.trim().to_string()),
    };
    let author_type = if author.eq_ignore_ascii_case("user") {
        AuthorType::Human
    } else {
        AuthorType::Agent
    };
    FeedbackReply {
        author,
        author_type,
        body,
        created_at: String::new(),
    }
}

/// List a run's feedback items for `station`, each carrying its reply thread.
///
/// Reads the run's feedback sidecar files off `.darkrun/` and returns the
/// parsed items filtered to the requested station. Items with no recorded
/// station are treated as belonging to every station (legacy-tolerant), and the
/// result is sorted by feedback id.
pub fn feedback_for_station(store: &StateStore, run: &str, station: &str) -> FeedbackListResponse {
    let raw = store.read_feedback_raw(run).unwrap_or_default();
    let mut items: Vec<FeedbackItem> = raw
        .into_iter()
        .map(|(id, content)| FeedbackDoc::parse(&id, &content))
        .filter(|doc| doc.matches_station(station))
        .map(|doc| {
            let mut item = doc.to_item();
            // `to_item` projects the frontmatter; the reply thread rides along
            // here so the list payload is the full record.
            item.replies = doc.replies.iter().map(|r| wire_reply(r)).collect();
            item
        })
        .collect();
    items.sort_by(|a, b| a.feedback_id.cmp(&b.feedback_id));

    FeedbackListResponse {
        run: run.to_string(),
        station: station.to_string(),
        count: items.len(),
        items,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_reply_attributes_author_and_falls_back_to_user() {
        let agent = wire_reply("worker-3: bounced this back");
        assert_eq!(agent.author, "worker-3");
        assert_eq!(agent.author_type, AuthorType::Agent);
        assert_eq!(agent.body, "bounced this back");
        let untagged = wire_reply("no prefix here");
        assert_eq!(untagged.author, "user");
        assert_eq!(untagged.author_type, AuthorType::Human);
    }

    #[test]
    fn feedback_for_station_filters_and_sorts() {
        let repo = tempfile::tempdir().unwrap();
        let store = StateStore::new(repo.path());
        let build = FeedbackDoc::new_user("FB-02".into(), "build".into(), "b".into(), "body".into());
        let frame = FeedbackDoc::new_user("FB-01".into(), "frame".into(), "f".into(), "body".into());
        store.write_feedback_raw("run-1", "FB-02", &build.render()).unwrap();
        store.write_feedback_raw("run-1", "FB-01", &frame.render()).unwrap();

        let resp = feedback_for_station(&store, "run-1", "build");
        assert_eq!(resp.count, 1);
        assert_eq!(resp.items[0].feedback_id, "FB-02");
        assert_eq!(resp.station, "build");
    }
}
