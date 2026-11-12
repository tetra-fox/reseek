//! Resuming the downloads a stop interrupted, once a login can reach their users: each is
//! enqueued again in its job, with its destination and search, as a new download that
//! supersedes the old record. The files of a group refused for want of the server keep their
//! marks and their job stays open, so the next login enqueues them. A job held so is let go of
//! by the next pass that does not find its files, which were removed meanwhile, and by a cancel
//! of the job.

use reseek_shares::lock;
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
  /// the next login makes; once every group was tried logged in, a call does nothing. A user
  /// that cannot be reached is logged, not tried again.
  pub async fn resume(&self) {
    let mut done = self.resuming.lock().await;
    if *done {
      return;
    }
    let resumable = match cleanup::take_resumable(&self.db).await {
      Ok(resumable) => resumable,
      Err(e) => return warn!(error = %e, "interrupted downloads not resumed"),
    };
    // the jobs earlier passes held; each this pass does not take again is let go of after it
    let mut earlier = lock(&self.held).clone();
    for (job, groups) in resumable {
      let groups = self.whole_apart(groups).await;
      self.ensure_listed(job, &groups).await;
      // held open through the enqueue, so it does not end while some of its files wait
      if !earlier.remove(&job) {
        self.open_job(job);
      }
      let asked: Vec<(String, Vec<String>)> = groups
        .iter()
        .map(|g| {
          let files = g.files.iter().map(|f| f.filename.clone()).collect();
          (g.username.clone(), files)
        })
        .collect();
      let results = self.enqueue(job, groups, false).await;
      let mut kept = false;
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
            if let Err(e) = cleanup::keep_resumable(&self.db, job, &username, files).await {
              warn!(username, %job, error = %e, "interrupted downloads not kept for the next login");
              continue;
            }
            info!(
              username,
              %job,
              error = refused.message(),
              "interrupted downloads wait for the next login"
            );
            kept = true;
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
      }
      if kept {
        lock(&self.held).insert(job);
      } else {
        lock(&self.held).remove(&job);
        self.close_job(job).await;
      }
    }
    // held for files that were removed before a login could enqueue them
    for job in earlier {
      lock(&self.held).remove(&job);
      info!(%job, "download job held for a login let go; its files were removed");
      self.close_job(job).await;
    }
    if !lock(&self.held).is_empty() {
      return;
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
                api_types::jobs::JobEnding::Failed,
                Some("Nothing was queued".to_string()),
              )
              .await;
          }
        }
      }
      Err(e) => warn!(error = %e, "download jobs not checked after the restart"),
    }
  }

  /// Lets go of the job `job` when a resume holds it for files that wait for a login: their
  /// marks are cleared, so no login enqueues them, and the job is closed, which ends it as its
  /// downloads ended. A resume under way is waited for, as it may be taking those files.
  pub(super) async fn let_go(&self, job: Uuid) {
    if !lock(&self.held).contains(&job) {
      return;
    }
    let _resuming = self.resuming.lock().await;
    // the resume waited for may have let go of it, or enqueued its files
    if !lock(&self.held).remove(&job) {
      return;
    }
    if let Err(e) = cleanup::forget_resumable(&self.db, job).await {
      warn!(%job, error = %e, "files of a canceled job kept for the next login");
    }
    info!(%job, "download job held for a login let go");
    self.close_job(job).await;
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
