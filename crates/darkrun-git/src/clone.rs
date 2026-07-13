//! Cloning a remote repository.
//!
//! The rest of this crate is deliberately network-free — worktree and status
//! queries against a repo that already exists locally. Cloning is the one
//! operation that reaches the network, driven in-process by gitoxide's
//! pure-Rust clone (reqwest + rustls for HTTPS) — no `git` CLI, no C.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use crate::error::{GitError, Result};
use crate::Git;

/// Map any gix error into our crate error.
fn gix_err(e: impl std::fmt::Display) -> GitError {
    GitError::Gix(e.to_string())
}

/// Clone `url` into `dest`, returning a [`Git`] facade open on the clone.
///
/// `dest` is the working-tree directory to create (e.g. `~/darkrun/<repo>`); it
/// must not already exist as a non-empty directory, and its parent is created
/// if missing. On success the destination holds the checked-out repo and the
/// returned [`Git`] is opened against it.
///
/// A picked repository is usually PRIVATE, so this authenticates the clone the
/// same way [`fetch`](crate::GixBackend) and [`push`](crate::push) do: gix's HTTP
/// transport reads basic-auth from the URL's userinfo, so if
/// [`credentials_for`](crate::push::credentials_for) yields a token for the URL's
/// provider (from the darkrun credential store or the environment), it is injected
/// into the clone URL via `set_user`/`set_password`, identical to the proven
/// fetch/push path, only serialized into the clone URL because
/// [`gix::prepare_clone`] consumes a URL rather than a live remote. A public repo,
/// or a local/file/ssh source (`credentials_for` returns `None` for anything that
/// is not HTTP(S)), clones from the raw `url` byte-for-byte, exactly as before.
///
/// After an AUTHENTICATED clone the token is STRIPPED back out of the persisted
/// `origin` in `.git/config` (see [`strip_origin_userinfo`]): gix writes the clone
/// URL verbatim, so the token would otherwise sit in plaintext on disk, and
/// fetch/push re-inject credentials from the store on every call, so the saved
/// origin needs no embedded token at all.
///
/// Surfaces a [`GitError::Gix`] carrying gitoxide's message when the clone fails
/// (bad URL, auth failure, network down), so callers can show the operator the
/// real reason instead of a generic error.
#[cfg(not(tarpaulin_include))] // clones a repo over the network — irreducible I/O
pub fn clone_repo(url: &str, dest: &Path) -> Result<Git> {
    // Create the parent so a target like `~/darkrun/<repo>` works on a fresh
    // machine where `~/darkrun` doesn't exist yet. gix creates `dest` itself.
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|source| GitError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    // Resolve HTTPS credentials the same way fetch/push do, and (only when there
    // are any) serialize an authenticated clone URL carrying the token as
    // userinfo. `None` here means an unauthenticated clone from the raw `url`:
    // the original behavior, preserved byte-for-byte for public/local/ssh sources.
    let authed_url: Option<String> = gix::url::parse(url.into()).ok().and_then(|mut parsed| {
        let account = crate::push::credentials_for(&parsed)?;
        parsed.set_user(Some(account.username));
        parsed.set_password(Some(account.password));
        Some(parsed.to_bstring().to_string())
    });
    let clone_target: &str = authed_url.as_deref().unwrap_or(url);

    // Fetch into a fresh repo at `dest`, then check out its main worktree. Under
    // `blocking-network-client` these are synchronous (maybe_async-stripped).
    let interrupt = AtomicBool::new(false);
    let mut fetch = gix::prepare_clone(clone_target, dest).map_err(gix_err)?;
    let (mut checkout, _) = fetch
        .fetch_then_checkout(gix::progress::Discard, &interrupt)
        .map_err(gix_err)?;
    checkout
        .main_worktree(gix::progress::Discard, &interrupt)
        .map_err(gix_err)?;

    // The token must never persist on disk: scrub the userinfo out of the origin
    // gix just wrote. Only meaningful when we injected one; a plain clone's origin
    // has no userinfo to strip, so this only runs on the authenticated path.
    if authed_url.is_some() {
        strip_origin_userinfo(dest)?;
    }

    // Open the freshly-cloned tree so the caller gets a ready-to-use facade.
    Git::open(dest)
}

/// Remove any HTTPS basic-auth userinfo (`user:pass@`) from the `origin` URL
/// persisted in `dest/.git/config`, rewriting the file in place.
///
/// An authenticated clone leaves gix's clone URL (token and all) in the config's
/// `[remote "origin"] url = https://<user>:<token>@host/...`; this reads the
/// config text, runs it through the pure [`strip_config_url_userinfo`] helper, and
/// writes it back only if it changed. A missing `.git/config` (there should always
/// be one on a fresh clone) is treated as a no-op rather than failing the clone.
fn strip_origin_userinfo(dest: &Path) -> Result<()> {
    let config_path = dest.join(".git").join("config");
    let text = match std::fs::read_to_string(&config_path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(GitError::Io {
                path: config_path,
                source,
            })
        }
    };
    let cleaned = strip_config_url_userinfo(&text);
    if cleaned != text {
        std::fs::write(&config_path, cleaned).map_err(|source| GitError::Io {
            path: config_path,
            source,
        })?;
    }
    Ok(())
}

/// Strip HTTP(S) basic-auth userinfo out of every `url = ` entry in a git config's
/// text, returning the rewritten text (pure: no I/O, so it is unit-tested
/// directly).
///
/// Git writes a remote's URL as an indented `url = <value>` entry under its
/// `[remote "…"]` section; only those lines can carry an origin token. For each
/// such line the value (after the first `=`) is passed through
/// [`strip_url_userinfo`], with its surrounding indentation, key, leading spaces,
/// and trailing newline all preserved; every other line is returned untouched.
fn strip_config_url_userinfo(config_text: &str) -> String {
    config_text
        .split_inclusive('\n')
        .map(|line| {
            let key = line.trim_start();
            if !(key.starts_with("url ") || key.starts_with("url=")) {
                return line.to_string();
            }
            let Some(eq) = line.find('=') else {
                return line.to_string();
            };
            // Split off `<indent>url =`, then peel the value's leading spaces and
            // trailing line terminator so `strip_url_userinfo` sees a bare URL.
            let (head, raw_value) = line.split_at(eq + 1);
            let lead_len = raw_value.len() - raw_value.trim_start().len();
            let (lead, after_lead) = raw_value.split_at(lead_len);
            let value = after_lead.trim_end_matches(['\n', '\r']);
            let trail = &after_lead[value.len()..];
            format!("{head}{lead}{}{trail}", strip_url_userinfo(value))
        })
        .collect()
}

/// Remove the `userinfo@` segment from a single `scheme://userinfo@host/…` URL,
/// returning the input unchanged when there is nothing to strip.
///
/// The strip is deliberately restricted to `http`/`https` authorities: those are
/// the ONLY schemes [`credentials_for`](crate::push::credentials_for) ever embeds
/// a token in, and an ssh `git@host` userinfo is load-bearing (dropping it breaks
/// the remote), so an ssh/file/scp-style value is left alone. Within the authority
/// (everything between `://` and the next `/`, `?`, `#`, or end) the last `@` is
/// the userinfo separator; everything before it (and the `@`) is dropped. A URL
/// with no `://`, a non-HTTP(S) scheme, or no userinfo is returned unchanged.
fn strip_url_userinfo(url: &str) -> String {
    let Some(scheme_end) = url.find("://") else {
        return url.to_string();
    };
    let scheme = &url[..scheme_end];
    if !scheme.eq_ignore_ascii_case("https") && !scheme.eq_ignore_ascii_case("http") {
        return url.to_string();
    }
    let authority_start = scheme_end + 3;
    // The authority runs until the first path/query/fragment delimiter (or end),
    // so a later `@` in the path can never be mistaken for the userinfo separator.
    let authority_end = url[authority_start..]
        .find(['/', '?', '#'])
        .map(|i| authority_start + i)
        .unwrap_or(url.len());
    let authority = &url[authority_start..authority_end];
    let Some(at) = authority.rfind('@') else {
        return url.to_string();
    };
    format!(
        "{}{}{}",
        &url[..authority_start],
        &authority[at + 1..],
        &url[authority_end..],
    )
}

/// Derive the default clone target directory name from a git `url`.
///
/// Strips a trailing `.git` and any path/query, yielding the repo's basename —
/// the name `git clone <url>` would itself pick. Used to default the clone
/// destination to `~/darkrun/<name>`. Falls back to `"repo"` for a URL with no
/// recoverable basename.
pub fn repo_name_from_url(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches('/');
    // Split on both `/` (https / path) and `:` (scp-style `git@host:owner/repo`).
    let tail = trimmed
        .rsplit(['/', ':'])
        .next()
        .unwrap_or(trimmed);
    let name = tail.strip_suffix(".git").unwrap_or(tail);
    if name.is_empty() {
        "repo".to_string()
    } else {
        name.to_string()
    }
}

/// The default clone destination for `url` under `base` — `<base>/<repo-name>`.
///
/// `base` is the editable clone-root the desktop defaults to `~/darkrun`. The
/// caller may override the returned path before cloning.
pub fn default_clone_dest(base: &Path, url: &str) -> PathBuf {
    base.join(repo_name_from_url(url))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GitBackend;
    use std::process::Command;
    use tempfile::TempDir;

    #[test]
    fn repo_name_strips_git_suffix_and_path() {
        assert_eq!(repo_name_from_url("https://github.com/acme/store.git"), "store");
        assert_eq!(repo_name_from_url("https://github.com/acme/store"), "store");
        assert_eq!(repo_name_from_url("git@github.com:acme/store.git"), "store");
        assert_eq!(repo_name_from_url("https://example.com/store/"), "store");
        assert_eq!(repo_name_from_url(""), "repo");
    }

    #[test]
    fn default_dest_joins_base_and_name() {
        let dest = default_clone_dest(Path::new("/home/me/darkrun"), "https://x/y/proj.git");
        assert_eq!(dest, PathBuf::from("/home/me/darkrun/proj"));
    }

    #[test]
    fn clone_from_local_source_repo() {
        // Build a throwaway source repo with one commit, then clone it from a
        // local path (no network) and verify the clone opens cleanly.
        let src_dir = TempDir::new().unwrap();
        let src = src_dir.path().to_path_buf();
        let git = |cwd: &Path, args: &[&str]| {
            let status = Command::new("git")
                .arg("-C")
                .arg(cwd)
                .args(args)
                .status()
                .expect("run git");
            assert!(status.success(), "git {args:?} failed");
        };
        git(&src, &["init", "-q", "-b", "main"]);
        git(&src, &["config", "user.email", "test@darkrun.ai"]);
        git(&src, &["config", "user.name", "darkrun test"]);
        std::fs::write(src.join("README.md"), "# src\n").unwrap();
        git(&src, &["add", "-A"]);
        git(&src, &["commit", "-q", "-m", "init"]);

        let work = TempDir::new().unwrap();
        let dest = work.path().join("nested").join("clone");
        let cloned = clone_repo(&src.to_string_lossy(), &dest).expect("clone");

        assert!(dest.join("README.md").exists(), "clone should have content");
        assert_eq!(cloned.repo_root(), dest.as_path());
        assert_eq!(cloned.current_branch().unwrap().as_deref(), Some("main"));
    }

    #[test]
    fn clone_bad_url_surfaces_an_error() {
        let work = TempDir::new().unwrap();
        let dest = work.path().join("nope");
        match clone_repo("/this/path/does/not/exist.git", &dest) {
            Err(GitError::Gix(_)) => {}
            Err(other) => panic!("expected a gix error, got {other:?}"),
            Ok(_) => panic!("clone of a nonexistent source should fail"),
        }
    }

    #[test]
    fn strip_url_userinfo_removes_only_https_userinfo() {
        // An HTTPS token is scrubbed, leaving a clean URL.
        assert_eq!(
            strip_url_userinfo("https://x-access-token:ghtok@github.com/o/r.git"),
            "https://github.com/o/r.git"
        );
        // http too (the scheme is allowed).
        assert_eq!(strip_url_userinfo("http://u:p@host/x"), "http://host/x");
        // No userinfo → unchanged.
        assert_eq!(
            strip_url_userinfo("https://github.com/o/r.git"),
            "https://github.com/o/r.git"
        );
        // An ssh userinfo is load-bearing and must survive.
        assert_eq!(strip_url_userinfo("ssh://git@github.com/o/r.git"), "ssh://git@github.com/o/r.git");
        // A scp-style / no-scheme value has no `://` and is left alone.
        assert_eq!(strip_url_userinfo("git@github.com:o/r.git"), "git@github.com:o/r.git");
        // A `@` in the path (after the authority) is not mistaken for userinfo.
        assert_eq!(
            strip_url_userinfo("https://github.com/o/r@v1.git"),
            "https://github.com/o/r@v1.git"
        );
    }

    #[test]
    fn strip_config_url_userinfo_scrubs_url_lines_only() {
        // A cloned origin carrying a token is sanitized; the sibling `fetch` line
        // and the section header are untouched, and the trailing newlines survive.
        let with = "[remote \"origin\"]\n\turl = https://x-access-token:ghtok@github.com/o/r.git\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n";
        let cleaned = strip_config_url_userinfo(with);
        assert!(cleaned.contains("\turl = https://github.com/o/r.git\n"), "{cleaned}");
        assert!(!cleaned.contains("ghtok"));
        assert!(cleaned.contains("\tfetch = +refs/heads/*:refs/remotes/origin/*\n"));
        assert!(cleaned.contains("[remote \"origin\"]\n"));

        // No userinfo anywhere → byte-for-byte unchanged.
        let clean = "[remote \"origin\"]\n\turl = https://github.com/o/r.git\n";
        assert_eq!(strip_config_url_userinfo(clean), clean);

        // A non-HTTPS (ssh) url line keeps its load-bearing `git@` user.
        let ssh = "\turl = ssh://git@github.com/o/r.git\n";
        assert_eq!(strip_config_url_userinfo(ssh), ssh);
    }

    #[test]
    fn local_clone_origin_carries_no_userinfo() {
        // A local (file) source authenticates with nothing (`credentials_for`
        // returns None for non-HTTPS), so the clone stays on the plain path and
        // its persisted origin is already clean: the regression guard that the
        // auth/strip machinery never corrupts an unauthenticated clone.
        let src_dir = TempDir::new().unwrap();
        let src = src_dir.path().to_path_buf();
        let git = |cwd: &Path, args: &[&str]| {
            let status = Command::new("git")
                .arg("-C")
                .arg(cwd)
                .args(args)
                .status()
                .expect("run git");
            assert!(status.success(), "git {args:?} failed");
        };
        git(&src, &["init", "-q", "-b", "main"]);
        git(&src, &["config", "user.email", "test@darkrun.ai"]);
        git(&src, &["config", "user.name", "darkrun test"]);
        std::fs::write(src.join("README.md"), "# src\n").unwrap();
        git(&src, &["add", "-A"]);
        git(&src, &["commit", "-q", "-m", "init"]);

        let work = TempDir::new().unwrap();
        let dest = work.path().join("clone");
        clone_repo(&src.to_string_lossy(), &dest).expect("clone");

        let config = std::fs::read_to_string(dest.join(".git").join("config")).expect("read config");
        // Every `url = ` entry is already free of userinfo (strip is a no-op).
        for line in config.lines() {
            if let Some(value) = line.trim_start().strip_prefix("url = ") {
                assert_eq!(
                    strip_url_userinfo(value),
                    value,
                    "a plain clone's origin must carry no userinfo: {value}"
                );
            }
        }
    }
}
