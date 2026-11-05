//! Answering a search from the share index. Each term is a phrase every match must hold and
//! each exclusion a phrase no match may hold, matched against the words of the whole remote
//! name, directories included, with case and Latin accents ignored. A term with no word in it,
//! such as `-` in `Beatles - Abbey Road`, is dropped rather than matching nothing, and an
//! exclusion also removes every match whose name holds it anywhere, ignoring case.

use rusqlite::{Connection, params};
use soulseek_proto::SearchQuery;
use soulseek_proto::peer;

use super::repository::shared_file;
use super::visibility::View;

/// Whether the filename index takes `c` as part of a word: letters, numbers and private-use
/// characters; everything else separates words.
fn word_char(c: char) -> bool {
  c.is_alphanumeric()
    || matches!(
      c,
      '\u{E000}'..='\u{F8FF}' | '\u{F0000}'..='\u{FFFFD}' | '\u{100000}'..='\u{10FFFD}'
    )
}

/// A term or exclusion as an index phrase: the characters that could end the phrase or start
/// a column filter made spaces, then quoted. `None` when it holds no word, so as a phrase it
/// would match nothing.
fn phrase(text: &str) -> Option<String> {
  let cleaned: String = text
    .chars()
    .map(|c| {
      if matches!(c, '/' | '\\' | ':' | '"') {
        ' '
      } else {
        c
      }
    })
    .collect();
  cleaned
    .chars()
    .any(word_char)
    .then(|| format!("\"{cleaned}\""))
}

/// The index expression for `query`, `None` when no term holds a word.
pub fn expression(query: &SearchQuery) -> Option<String> {
  let terms: Vec<String> = query.terms.iter().filter_map(|t| phrase(t)).collect();
  if terms.is_empty() {
    return None;
  }
  let exclusions: Vec<String> = query.exclusions.iter().filter_map(|e| phrase(e)).collect();
  let terms = terms.join(" AND ");
  Some(if exclusions.is_empty() {
    format!("({terms})")
  } else {
    format!("({terms}) NOT ({})", exclusions.join(" OR "))
  })
}

/// The files matching `query` that `view` sees, ordered by remote name, each named by its whole
/// remote name; at most `limit` of them, counted after the exclusions and the view are applied.
pub fn search(
  conn: &Connection,
  query: &SearchQuery,
  limit: Option<usize>,
  view: &View,
) -> rusqlite::Result<Vec<peer::File>> {
  let Some(expression) = expression(query) else {
    return Ok(Vec::new());
  };
  let excluded: Vec<String> = query
    .exclusions
    .iter()
    .filter(|e| !e.is_empty())
    .map(|e| e.to_lowercase())
    .collect();
  run(conn, &expression, &excluded, limit.unwrap_or(usize::MAX), view)
}

fn run(
  conn: &Connection,
  expression: &str,
  excluded: &[String],
  limit: usize,
  view: &View,
) -> rusqlite::Result<Vec<peer::File>> {
  let mut statement = conn.prepare_cached(
    "SELECT files.maskedFilename, files.size, files.extension, files.attributeJson \
     FROM filenames INNER JOIN files ON filenames.rowid = files.id \
     WHERE filenames MATCH ?1 ORDER BY files.maskedFilename",
  )?;
  let mut rows = statement.query(params![expression])?;
  let mut files = Vec::new();
  while files.len() < limit
    && let Some(row) = rows.next()?
  {
    let masked: String = row.get(0)?;
    let lower = masked.to_lowercase();
    if view.hides(&masked) || excluded.iter().any(|e| lower.contains(e.as_str())) {
      continue;
    }
    files.push(shared_file(
      &masked,
      row.get(1)?,
      row.get(2)?,
      &row.get::<_, String>(3)?,
    ));
  }
  Ok(files)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::repository::tests::{index, write};

  fn query(terms: &[&str], exclusions: &[&str]) -> SearchQuery {
    SearchQuery {
      terms: terms.iter().map(|t| t.to_string()).collect(),
      exclusions: exclusions.iter().map(|e| e.to_string()).collect(),
    }
  }

  #[test]
  fn terms_without_words_are_dropped_and_operators_are_literal() {
    assert_eq!(
      expression(&query(&["Beatles", "-", "Abbey"], &[])).as_deref(),
      Some("(\"Beatles\" AND \"Abbey\")")
    );
    assert_eq!(expression(&query(&["-", "", "()"], &[])), None);
    assert_eq!(
      expression(&query(&["a\"b", "x:y"], &["c/d", "..."])).as_deref(),
      Some("(\"a b\" AND \"x y\") NOT (\"c d\")"),
      "an exclusion without words leaves the index expression"
    );
  }

  #[tokio::test]
  async fn matches_are_by_word_with_case_and_accents_folded() {
    let db = index().await;
    let found = db
      .write(|conn| {
        write(
          conn,
          1,
          &[
            (
              "Music\\Beatles\\Abbey Road",
              &[
                "Music\\Beatles\\Abbey Road\\01 - Come Together.mp3",
                "Music\\Beatles\\Abbey Road\\02 Something (live).mp3",
              ],
            ),
            ("Music\\Björk", &["Music\\Björk\\Début.flac"]),
          ],
        );
        let names = |q: SearchQuery, limit| {
          search(conn, &q, limit, &View::everything())
            .unwrap()
            .into_iter()
            .map(|f| f.filename.as_str().to_string())
            .collect::<Vec<_>>()
        };
        Ok((
          names(query(&["beatles", "-", "abbey", "road"], &[]), None),
          names(query(&["bjork", "debut"], &[]), None),
          names(query(&["road", "abbey"], &[]), None),
          names(query(&["\"road abbey\""], &[]), None),
          names(query(&["abbey"], &["live"]), None),
          names(query(&["music"], &["ETH"]), None),
          names(query(&["1"], &[]), None),
          names(query(&["together.mp3"], &[]), None),
          names(query(&["abbey"], &["come"]), Some(1)),
          names(query(&["NEAR(a"], &[]), None),
        ))
      })
      .await
      .unwrap();
    assert_eq!(found.0.len(), 2, "every term anywhere in the path");
    assert_eq!(found.1, ["Music\\Björk\\Début.flac"]);
    assert_eq!(found.2.len(), 2, "terms in any order");
    assert!(found.3.is_empty(), "a phrase keeps its order");
    assert_eq!(
      found.4,
      ["Music\\Beatles\\Abbey Road\\01 - Come Together.mp3"]
    );
    assert_eq!(
      found.5,
      ["Music\\Björk\\Début.flac"],
      "an exclusion removes a name holding it anywhere"
    );
    assert!(found.6.is_empty(), "01 is not 1");
    assert_eq!(
      found.7.len(),
      1,
      "a term may span words that follow each other"
    );
    assert_eq!(
      found.8,
      ["Music\\Beatles\\Abbey Road\\02 Something (live).mp3"],
      "the limit counts what the exclusions leave"
    );
    assert!(found.9.is_empty(), "an operator in a term is text");
  }
}
