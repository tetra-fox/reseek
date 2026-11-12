//! Resuming the downloads a stop interrupted, once a login can reach their users: each is
//! enqueued again in its job, with its destination and search, as a new download that
//! supersedes the old record. What says a download still waits for this is its resumable mark
//! in the database, cleared once its file was tried; a file refused for want of the server
//! keeps it, so the next login tries it, and its job stays running meanwhile. A cancel of the
//! job clears the marks, so no login enqueues its files.

use api_types::jobs::JobEnding;
use tracing::{info, warn};
use uuid::Uuid;

use super::{Downloads, Group};
use crate::transfers::cleanup;

impl Downloads {
  /// Enqueues again the downloads a stop interrupted that were not removed since, each in its
  /// job and with its destination and search, as new downloads that supersede the old records,
  /// so a job goes on and is told of once all of it has ended. A file whole here already is
  /// enqueued apart from the rest of its group, so it is moved whether or not its user can be
  /// reached. A group refused because we are not logged in is left for the next call, which
  /// the next login makes; once no download waits, a call does nothing. A user that cannot be
  /// reached is logged, not tried again.
  pub async fn resume(&self) {
    let mut done = self.resuming.lock().await;
    if *done {
      return;
    }
    let resumable = match cleanup::take_resumable(&self.db).await {
      Ok(resumable) => resumable,
      Err(e) => return warn!(error = %e, "interrupted downloads not resumed"),
    };
    for (job, groups) in resumable {
      let groups = self.whole_apart(groups).await;
      self.ensure_listed(job, &groups).await;
      // open through the enqueue and the marks cleared after it, so the job is checked once
      // its files are settled
      self.open_job(job);
      let asked: Vec<(String, Vec<String>)> = groups
        .iter()
        .map(|g| {
          let files = g.files.iter().map(|f| f.filename.clone()).collect();
          (g.username.clone(), files)
        })
        .collect();
      let results = self.enqueue(job, groups, false).await;
      for ((username, files), result) in asked.into_iter().zip(results) {
        match result {
          Ok(enqueued) => info!(
            username,
            %job,
            resumed = enqueued.enqueued.len(),
            failed = enqueued.failed.len(),
            "interrupted downloads resumed"
          ),
          Err(refused) if refused.is_unavailable() || !self.logged_in() => {
            info!(
              username,
              %job,
              error = refused.message(),
              "interrupted downloads wait for the next login"
            );
            continue;
          }
          // an offline user is no fault, so this logs as a refused enqueue does
          Err(e) => info!(
            username,
            %job,
            downloads = files.len(),
            error = e.message(),
            "interrupted downloads not resumed; the user is likely offline"
          ),
        }
        if let Err(e) = cleanup::clear_resumable(&self.db, job, &username, files).await {
          warn!(username, %job, error = %e, "interrupted downloads tried but still marked");
        }
      }
      self.close_job(job).await;
    }
    match cleanup::any_resumable(&self.db).await {
      Ok(false) => {}
      Ok(true) => return,
      Err(e) => return warn!(error = %e, "interrupted downloads left not read"),
    }
    *done = true;
    // a job that ended or lost its downloads while the list was not told stays in step
    match self.jobs.unended(api_types::jobs::JobKind::Download).await {
      Ok(jobs) => {
        for job in jobs {
          // one stopped before its first download was written ends with nothing to show
          if self.sync_job(job.id).await {
            self
              .end_empty(
                job.id,
                JobEnding::Failed,
                Some("Nothing was queued".to_string()),
              )
              .await;
          }
        }
      }
      Err(e) => warn!(error = %e, "download jobs not checked after the restart"),
    }
  }

  /// Gives up the files of the job `job` that wait for a login, clearing their marks so no
  /// login enqueues them; true when there were any. A resume under way is waited for, as it may
  /// be trying those files.
  pub(super) async fn give_up_waiting(&self, job: Uuid) -> bool {
    let _resuming = self.resuming.lock().await;
    match cleanup::forget_resumable(&self.db, job).await {
      Ok(waited) => {
        if waited {
          info!(%job, "files of a download job waiting for a login given up");
        }
        waited
      }
      Err(e) => {
        warn!(%job, error = %e, "files of a canceled job still wait for a login");
        false
      }
    }
  }

  /// Whether the client is logged in to the server.
  fn logged_in(&self) -> bool {
    self.network.client().state().is_logged_in()
  }

  /// `groups` with the files whole here already in a group of their own, ahead of the rest of
  /// the group they were in.
  async fn whole_apart(&self, groups: Vec<Group>) -> Vec<Group> {
    let mut split = Vec::with_capacity(groups.len());
    for mut group in groups {
      let mut whole = Vec::new();
      let mut rest = Vec::new();
      for file in group.files {
        if self.whole_here(&group.username, &file).await {
          whole.push(file);
        } else {
          rest.push(file);
        }
      }
      if !whole.is_empty() {
        split.push(Group {
          files: whole,
          username: group.username.clone(),
          destination: group.destination.clone(),
          search_id: group.search_id,
        });
      }
      if !rest.is_empty() {
        group.files = rest;
        split.push(group);
      }
    }
    split
  }
}
