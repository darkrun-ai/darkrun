//! HTTP/1.1 status-line parsing for the hand-rolled decision POST.

use darkrun_desktop::wire::{parse_status_code, WireError};

fn code(buf: &[u8]) -> u16 {
    parse_status_code(buf).expect("expected a parseable status")
}

// ---- 2xx ----

#[test]
fn parses_200() {
    assert_eq!(code(b"HTTP/1.1 200 OK\r\n\r\n{}"), 200);
}

#[test]
fn parses_201_created() {
    assert_eq!(code(b"HTTP/1.1 201 Created\r\n\r\n"), 201);
}

#[test]
fn parses_204_no_content() {
    assert_eq!(code(b"HTTP/1.1 204 No Content\r\n\r\n"), 204);
}

#[test]
fn parses_299_edge_of_success_band() {
    assert_eq!(code(b"HTTP/1.1 299 Whatever\r\n"), 299);
}

// ---- 4xx / 5xx ----

#[test]
fn parses_400() {
    assert_eq!(code(b"HTTP/1.1 400 Bad Request\r\n"), 400);
}

#[test]
fn parses_404() {
    assert_eq!(code(b"HTTP/1.1 404 Not Found\r\n"), 404);
}

#[test]
fn parses_409_conflict() {
    assert_eq!(code(b"HTTP/1.1 409 Conflict\r\n"), 409);
}

#[test]
fn parses_500() {
    assert_eq!(code(b"HTTP/1.1 500 Internal Server Error\r\n"), 500);
}

#[test]
fn parses_503() {
    assert_eq!(code(b"HTTP/1.1 503 Service Unavailable\r\n"), 503);
}

// ---- protocol/version variations ----

#[test]
fn parses_http_1_0() {
    assert_eq!(code(b"HTTP/1.0 200 OK\r\n"), 200);
}

#[test]
fn parses_http_2_style() {
    assert_eq!(code(b"HTTP/2 200\r\n"), 200);
}

#[test]
fn parses_status_with_no_reason_phrase() {
    assert_eq!(code(b"HTTP/1.1 200\r\n"), 200);
}

#[test]
fn parses_only_lf_line_ending() {
    // `lines()` handles bare-LF too.
    assert_eq!(code(b"HTTP/1.1 200 OK\n\nbody"), 200);
}

#[test]
fn parses_with_multiple_spaces() {
    // split_whitespace collapses runs of spaces; nth(1) is still the code.
    assert_eq!(code(b"HTTP/1.1   404   Not Found\r\n"), 404);
}

#[test]
fn parses_with_leading_tab_in_status_line() {
    // split_whitespace treats tabs as whitespace; first token is the version.
    assert_eq!(code(b"HTTP/1.1\t418 I'm a teapot\r\n"), 418);
}

#[test]
fn ignores_subsequent_header_lines() {
    let resp = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}";
    assert_eq!(code(resp), 200);
}

#[test]
fn body_containing_http_like_line_does_not_confuse_parser() {
    // Only the first line is parsed; a body line that looks like a status line
    // is irrelevant.
    let resp = b"HTTP/1.1 500 Err\r\n\r\nHTTP/1.1 200 OK";
    assert_eq!(code(resp), 500);
}

// ---- malformed -> Err(WireError::Io) ----

fn err(buf: &[u8]) -> WireError {
    parse_status_code(buf).expect_err("expected a parse error")
}

fn is_io(e: &WireError) -> bool {
    matches!(e, WireError::Io(_))
}

#[test]
fn empty_buffer_errors() {
    assert!(is_io(&err(b"")));
}

#[test]
fn garbage_errors() {
    assert!(is_io(&err(b"garbage")));
}

#[test]
fn single_token_errors() {
    // Only one whitespace token; nth(1) is None.
    assert!(is_io(&err(b"HTTP/1.1")));
}

#[test]
fn non_numeric_code_errors() {
    assert!(is_io(&err(b"HTTP/1.1 OK 200\r\n")));
}

#[test]
fn second_token_not_a_number_errors() {
    assert!(is_io(&err(b"HTTP/1.1 abc def\r\n")));
}

#[test]
fn negative_looking_code_errors() {
    // "-5" fails u16 parse.
    assert!(is_io(&err(b"HTTP/1.1 -5 Bad\r\n")));
}

#[test]
fn code_above_u16_errors() {
    // 70000 overflows u16.
    assert!(is_io(&err(b"HTTP/1.1 70000 Big\r\n")));
}

#[test]
fn whitespace_only_first_line_errors() {
    assert!(is_io(&err(b"   \r\nHTTP/1.1 200 OK")));
}

#[test]
fn empty_first_line_then_status_errors() {
    // First line is empty; only that line is parsed.
    assert!(is_io(&err(b"\r\nHTTP/1.1 200 OK\r\n")));
}

#[test]
fn malformed_error_message_is_descriptive() {
    let e = err(b"garbage");
    assert!(
        e.to_string().contains("malformed HTTP status"),
        "got: {e}"
    );
}

// ---- lossy UTF-8 handling ----

#[test]
fn invalid_utf8_bytes_handled_lossily() {
    // Non-UTF8 bytes after the status line shouldn't panic; the head is lossy.
    let mut buf = b"HTTP/1.1 200 OK\r\n".to_vec();
    buf.extend_from_slice(&[0xff, 0xfe, 0x80]);
    assert_eq!(code(&buf), 200);
}

#[test]
fn invalid_utf8_in_first_line_does_not_panic() {
    // Lossy decode replaces bad bytes; parsing still finds the code if present.
    let buf = vec![0xff, b' ', b'2', b'0', b'0', b' ', b'O', b'K'];
    // First token is the replacement char, second is "200".
    assert_eq!(code(&buf), 200);
}
