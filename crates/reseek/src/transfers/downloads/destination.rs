//! Where a download's file lives: the incomplete file it is written to, under a directory of
//! the peer's own, and the directory under the downloads directory it is moved to once it has
//! arrived, from the configured pattern or the download's own destination. Every name that came
//! from the peer, the destination or the search is made safe before it becomes part of a path.
//! The incomplete file's name carries a hash of the exact peer and remote filename, since two
//! downloads can have names that are made safe alike or that a filesystem takes as one.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::{Captures, Regex};
use reseek_shares::traverses;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::safety::{
  Os, combine_safely, creatable, creatable_path, directory_name_safely, file_name_safely,
  is_absolute, sanitize_filename, sanitize_path_segment, too_long, with_suffix,
};

/// How many bytes of the hash of a download's peer and remote filename its incomplete file's
/// name carries.
const KEY_BYTES: usize = 8;

/// The pattern used when none is configured: the remote file's own directory name.
const DEFAULT_PATTERN: &str = "${SOURCE_DIRECTORY}";

/// The pattern that puts files straight into the downloads directory.
const DOWNLOADS_ITSELF: &str = "{}";

/// One `${NAME}` in a pattern.
static TOKEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\$\{([^{}]*)\}").unwrap());

/// The search a download was found by, as the pattern reads it.
#[derive(Clone, Debug)]
pub struct SearchFound {
  /// the search's id
  pub id: Uuid,
  /// what was searched for
  pub text: String,
}

/// What a download's destination is derived from.
pub struct Source<'a> {
  /// the peer
  pub username: &'a str,
  /// the remote filename
  pub filename: &'a str,
  /// the download job the download was enqueued in
  pub job: Option<Uuid>,
  /// the download's own destination
  pub destination: Option<&'a str>,
  /// the search the download was found by, when it is still known
  pub search: Option<&'a SearchFound>,
}

/// A name from elsewhere as one safe segment, `/` and `\` replaced.
fn segment(name: &str, os: Os) -> String {
  // `_` is no separator and no character any system forbids, so it is never refused
  creatable(
    &sanitize_path_segment(name, '_', os).unwrap_or_default(),
    os,
  )
}

/// A name from elsewhere as one safe file name: `/` and `\` replaced, never a new directory.
fn file_name(name: &str, os: Os) -> String {
  // `_` is no separator and no character any system forbids, so it is never refused
  sanitize_filename(name, '_', os).unwrap_or_default()
}

/// The relative directory under the downloads directory the download of `source` goes to,
/// empty for the downloads directory itself: the download's own destination when it has one, else
/// `pattern` with its `${NAME}`s filled in, made safe. Fails only when the result would leave
/// the downloads directory, which making it safe rules out.
pub fn subdirectory(pattern: Option<&str>, source: &Source<'_>, os: Os) -> Result<String, String> {
  if let Some(destination) = source
    .destination
    .filter(|destination| !destination.trim().is_empty())
  {
    // the download's own destination is taken as written, its `${NAME}`s left as they are
    return contained(creatable_path(destination, os), os);
  }
  let pattern = pattern.unwrap_or(DEFAULT_PATTERN);
  if pattern.eq_ignore_ascii_case(DOWNLOADS_ITSELF) {
    return Ok(String::new());
  }
  let source_path = directory_name_safely(source.filename, true, os).unwrap_or_default();
  let source_directory = source_path
    .rsplit(os.separator())
    .next()
    .unwrap_or_default()
    .to_string();
  let value = |name: &str| -> Option<String> {
    Some(match name.to_ascii_uppercase().as_str() {
      "SOURCE_USERNAME" => file_name(source.username, os),
      "SOURCE_PATH" => source_path.clone(),
      "SOURCE_DIRECTORY" => source_directory.clone(),
      "JOB_ID" => source
        .job
        .filter(|id| !id.is_nil())
        .map_or_else(|| "unknown_job_id".to_string(), |id| id.to_string()),
      "SEARCH_ID" => source
        .search
        .map(|s| s.id)
        .filter(|id| !id.is_nil())
        .map_or_else(|| "unknown_search_id".to_string(), |id| id.to_string()),
      "SEARCH_TEXT" => source
        .search
        .map(|s| s.text.as_str())
        .filter(|text| !text.trim().is_empty())
        .map_or_else(
          || "unknown_search_text".to_string(),
          |text| file_name(text, os),
        ),
      _ => return None,
    })
  };
  // one pass, so a value holding a `${NAME}` is never filled in itself
  let filled = TOKEN.replace_all(pattern, |captures: &Captures<'_>| {
    value(&captures[1]).unwrap_or_else(|| captures[0].to_string())
  });
  contained(creatable_path(&filled, os), os)
}

/// `relative` when it stays under the directory it is joined to.
fn contained(relative: String, os: Os) -> Result<String, String> {
  if is_absolute(&relative, os) || traverses(&relative) {
    return Err(format!(
      "the destination {relative} would leave the downloads directory"
    ));
  }
  Ok(relative)
}

/// The text of a root directory, which the path rules read.
fn root_text(root: &Path) -> Result<&str, String> {
  root
    .to_str()
    .ok_or_else(|| format!("{} is not valid UTF-8", root.display()))
}

/// The name the file `filename` has once downloaded, made safe.
pub fn local_name(filename: &str, os: Os) -> Result<String, String> {
  file_name_safely(filename, os)
    .map(|name| creatable(&name, os))
    .ok_or_else(|| format!("the remote filename {filename} names no file"))
}

/// The first [`KEY_BYTES`] of the SHA-256 of the exact `username` and `filename`, in hex. The
/// username's length goes first so no two pairs hash the same bytes.
fn download_key(username: &str, filename: &str) -> String {
  let mut hasher = Sha256::new();
  hasher.update((username.len() as u64).to_le_bytes());
  hasher.update(username);
  hasher.update(filename);
  hasher.finalize()[..KEY_BYTES]
    .iter()
    .map(|b| format!("{b:02x}"))
    .collect()
}

/// The incomplete file a download of `filename` from `username` is written to: under
/// `incomplete`, a directory named after the peer, then the remote directories without their
/// root, then the file's name with ` [key]` from [`download_key`] before its extension, each
/// made safe. A path too long for `os` loses the remote directories, which is said by the
/// second value.
pub fn incomplete_path(
  incomplete: &Path,
  username: &str,
  filename: &str,
  os: Os,
) -> Result<(PathBuf, bool), String> {
  let key = download_key(username, filename);
  let name = with_suffix(&local_name(filename, os)?, &format!(" [{key}]"));
  placed(incomplete, username, filename, &name, os)
}

/// The incomplete file `name` of a download of `filename` from `username`, as
/// [`incomplete_path`] lays it out.
fn placed(
  incomplete: &Path,
  username: &str,
  filename: &str,
  name: &str,
  os: Os,
) -> Result<(PathBuf, bool), String> {
  let user = segment(username, os);
  let directories = directory_name_safely(filename, false, os)
    .map(|directories| creatable_path(&directories, os))
    .unwrap_or_default();
  let root = root_text(incomplete)?;
  let full = combine_safely(root, &[&user, &directories, name], os)?;
  if !too_long(&full, os) {
    return Ok((PathBuf::from(full), false));
  }
  Ok((
    PathBuf::from(combine_safely(root, &[&user, name], os)?),
    true,
  ))
}

/// The directory under `downloads` that the file `name` goes to for `subdirectory`, or, when
/// the file's path would be too long for `os`, a directory named after the peer, which is said
/// by the second value.
pub fn destination_directory(
  downloads: &Path,
  subdirectory: &str,
  username: &str,
  name: &str,
  os: Os,
) -> Result<(PathBuf, bool), String> {
  let root = root_text(downloads)?;
  let directory = combine_safely(root, &[subdirectory], os)?;
  let file = combine_safely(&directory, &[name], os)?;
  if !too_long(&file, os) {
    return Ok((PathBuf::from(directory), false));
  }
  let user = segment(username, os);
  Ok((PathBuf::from(combine_safely(root, &[&user], os)?), true))
}

#[cfg(test)]
mod tests {
  use super::*;

  fn source<'a>(username: &'a str, filename: &'a str) -> Source<'a> {
    Source {
      username,
      filename,
      job: None,
      destination: None,
      search: None,
    }
  }

  fn unix(pattern: &str, source: &Source<'_>) -> String {
    subdirectory(Some(pattern), source, Os::Unix).unwrap()
  }

  #[test]
  fn the_patterns_worked_examples() {
    let track = source("alice", "@alice\\Music\\album\\track.mp3");
    assert_eq!(unix("${SOURCE_DIRECTORY}", &track), "album");
    assert_eq!(unix("${SOURCE_PATH}", &track), "@alice/Music/album");
    assert_eq!(
      unix("${SOURCE_USERNAME}/downloads", &track),
      "alice/downloads"
    );
    let shallow = source("alice", "@alice\\track.mp3");
    assert_eq!(
      unix("${SOURCE_PATH}-${SOURCE_DIRECTORY}", &shallow),
      "@alice-@alice"
    );
    assert_eq!(
      unix("${source_directory}", &track),
      "album",
      "names ignore case"
    );
    assert_eq!(unix("{}", &track), "");
    assert_eq!(
      subdirectory(None, &track, Os::Unix).unwrap(),
      "album",
      "the source directory when none is set"
    );
  }

  #[test]
  fn hostile_names_never_leave_the_downloads_directory() {
    let file = "@@abcde\\Music\\track.mp3";
    assert_eq!(unix("${SOURCE_USERNAME}", &source("..", file)), "_");
    assert_eq!(
      unix("${SOURCE_USERNAME}", &source("al/../ice", file)),
      "al_.._ice"
    );
    assert_eq!(
      unix("${SOURCE_USERNAME}", &source("${SEARCH_ID}", file)),
      "${SEARCH_ID}",
      "a value is never filled in itself"
    );
    assert_eq!(
      unix("${NOT_A_TOKEN}/x", &source("a", file)),
      "${NOT_A_TOKEN}/x"
    );
    let root = source("a", "C:\\..\\..\\etc\\passwd");
    assert_eq!(unix("${SOURCE_PATH}", &root), "_/_/etc");
    assert_eq!(
      unix("../${SOURCE_DIRECTORY}/..", &source("a", file)),
      "_/Music/_"
    );
  }

  #[test]
  fn a_download_gives_its_own_job_search_and_destination() {
    let track = "@@abcde\\Music\\album\\01.flac";
    let explicit = Source {
      destination: Some("Artist/../Album/${SOURCE_USERNAME}"),
      ..source("alice", track)
    };
    assert_eq!(
      unix("${SOURCE_DIRECTORY}", &explicit),
      "Artist/_/Album/${SOURCE_USERNAME}",
      "taken as written, made safe"
    );
    assert_eq!(
      unix("{}", &explicit),
      "Artist/_/Album/${SOURCE_USERNAME}",
      "the download's own destination is taken under the pattern {{}} too"
    );
    let found = SearchFound {
      id: "0b5d7f91-1111-4222-8333-944455566677".parse().unwrap(),
      text: "AC/DC live".to_string(),
    };
    let queued = Source {
      job: Some("a3b5d7f9-1111-4222-8333-944455566677".parse().unwrap()),
      search: Some(&found),
      ..source("alice", track)
    };
    assert_eq!(
      unix("${JOB_ID}/${SEARCH_ID}/${SEARCH_TEXT}", &queued),
      "a3b5d7f9-1111-4222-8333-944455566677/0b5d7f91-1111-4222-8333-944455566677/AC_DC live"
    );
    let blank = Source {
      destination: Some("  "),
      ..queued
    };
    assert_eq!(
      unix("${SOURCE_DIRECTORY}", &blank),
      "album",
      "a blank destination is the pattern's"
    );
    let none = source("alice", "a.flac");
    assert_eq!(
      unix("${JOB_ID}-${SEARCH_ID}-${SEARCH_TEXT}", &none),
      "unknown_job_id-unknown_search_id-unknown_search_text"
    );
    let forgotten = SearchFound {
      id: Uuid::nil(),
      text: " ".to_string(),
    };
    let searched = Source {
      search: Some(&forgotten),
      ..none
    };
    assert_eq!(
      unix("${SEARCH_ID}/${SEARCH_TEXT}", &searched),
      "unknown_search_id/unknown_search_text"
    );
  }

  #[test]
  fn the_incomplete_file_is_under_the_peers_own_directory_without_the_remote_root() {
    let (path, short) = incomplete_path(
      Path::new("/data/incomplete"),
      "FooBar",
      "C:\\Music\\Public_Domain\\Foo\\Bar\\Baz.ext",
      Os::Unix,
    )
    .unwrap();
    assert_eq!(
      path,
      Path::new("/data/incomplete/FooBar/Music/Public_Domain/Foo/Bar/Baz [74147747ae8a619f].ext"),
      "the key never changes, or partials from before a change are not resumed"
    );
    assert!(!short);
    let (hostile, _) = incomplete_path(
      Path::new("/data/incomplete"),
      "../..",
      "@@abcde\\..\\..\\..\\secret",
      Os::Unix,
    )
    .unwrap();
    assert_eq!(
      hostile,
      Path::new("/data/incomplete/.._../_/_/_/secret [2b0a1ad417e7fe23]")
    );
    assert!(incomplete_path(Path::new("/in"), "a", "dir\\", Os::Unix).is_err());
  }

  #[test]
  fn downloads_whose_names_are_made_safe_alike_never_share_an_incomplete_file() {
    let album = "@@abcde\\Album\\01.flac";
    let pairs = [
      (("a/b", album), ("a_b", album)),
      (("Alice", album), ("alice", album)),
      (
        ("alice", "@@abcde\\Artist\\Album\\01.flac"),
        ("alice", "@@abcde\\artist\\album\\01.flac"),
      ),
      (
        ("alice", "@@abcde\\Caf\u{e9}\\01.flac"),
        ("alice", "@@abcde\\Cafe\u{301}\\01.flac"),
      ),
    ];
    for ((user_a, file_a), (user_b, file_b)) in pairs {
      let path = |user, file| {
        let (path, _) = incomplete_path(Path::new("/in"), user, file, Os::Unix).unwrap();
        path.to_str().unwrap().to_lowercase()
      };
      assert_ne!(
        path(user_a, file_a),
        path(user_b, file_b),
        "{user_a} {file_a}"
      );
      // the key is ASCII, so no normalisation of the name can make two keys one
      assert_ne!(download_key(user_a, file_a), download_key(user_b, file_b));
    }
  }

  #[test]
  fn windows_names_are_made_creatable() {
    let (path, _) = incomplete_path(
      Path::new("C:\\data\\incomplete"),
      "CON",
      "@@abcde\\Music\\Album. \\NUL.flac",
      Os::Windows,
    )
    .unwrap();
    assert_eq!(
      path,
      Path::new("C:\\data\\incomplete\\CON_\\Music\\Album\\NUL_ [fc695bc4ebcfa25a].flac")
    );
  }

  #[test]
  fn a_path_too_long_falls_back_to_the_peers_directory() {
    let deep = format!("{}\\01.flac", vec!["d".repeat(200); 30].join("\\"));
    let (path, short) = incomplete_path(Path::new("/in"), "alice", &deep, Os::Unix).unwrap();
    let key = download_key("alice", &deep);
    assert_eq!(path, Path::new(&format!("/in/alice/01 [{key}].flac")));
    assert!(short);
    let long = vec!["d".repeat(200); 30].join("/");
    let (dir, short) =
      destination_directory(Path::new("/dl"), &long, "alice", "01.flac", Os::Unix).unwrap();
    assert_eq!(dir, Path::new("/dl/alice"));
    assert!(short);
    let (dir, short) =
      destination_directory(Path::new("/dl"), "", "alice", "01.flac", Os::Unix).unwrap();
    assert_eq!((dir.as_path(), short), (Path::new("/dl"), false));
  }
}
