//! Notification message text — the ONE source of truth for what "a gate needs
//! you" reads as, shared by the LOCAL OS notification (the engine) and the
//! REMOTE push (the host connector → relay → FCM). Pure, so it's unit-tested
//! without raising anything.

/// The notification title + body for a run reaching an operator gate. When
/// `station` is empty the body is generic. Both halves of "notify as the engine
/// ticks" render the same text from here.
pub fn gate_message(run: &str, station: &str) -> (String, String) {
    let title = format!("darkrun · {run}");
    let body = if station.is_empty() {
        "A checkpoint needs your decision.".to_string()
    } else {
        let mut chars = station.chars();
        let station = chars
            .next()
            .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
            .unwrap_or_default();
        format!("{station} needs your decision.")
    };
    (title, body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_the_run_and_capitalizes_the_station() {
        let (title, body) = gate_message("quiet-canyon", "build");
        assert_eq!(title, "darkrun · quiet-canyon");
        assert_eq!(body, "Build needs your decision.");
    }

    #[test]
    fn handles_an_empty_station() {
        let (_t, body) = gate_message("r", "");
        assert_eq!(body, "A checkpoint needs your decision.");
    }
}
