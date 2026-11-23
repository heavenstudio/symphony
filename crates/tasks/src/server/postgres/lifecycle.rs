//! PostgreSQL's stopped-cluster snapshots and holds. A snapshot carries the complete cluster,
//! WAL included, and is published by rename only once copied. Recovery copies it into a new
//! scratch board; the snapshot itself is never started or changed. Physical recovery requires
//! the same PostgreSQL major. Routine free space is reused by autovacuum; `reclaim` asks
//! PostgreSQL to return it to the filesystem with VACUUM FULL, without exporting or swapping data.

use super::{DATA, Postgres, Serving, fs, io, output, server_command};
use crate::server::snapshot::{clone, copy, named, order, stamp};
use crate::server::starters::Doing;
use crate::server::{Hold, ServerError};
use std::path::{Path, PathBuf};

const SNAPSHOTS: &str = "postgresql-snapshots";
const TAKING: &str = ".taking";
/// The work folder of a restore over a board: the snapshot's copy, served there once to be
/// given its leases ([`Postgres::fenced`]).
const RESTORING: &str = "restoring";

/// A PostgreSQL board held stopped, under the same holder/release protocol as any board.
#[derive(Debug, Clone, PartialEq)]
pub struct Holding {
    pub hold: Hold,
    pub stopped: Option<Serving>,
    pub data: PathBuf,
}

impl Postgres {
    /// Hold the board stopped. The hold is recorded before shutdown, so a failed stop leaves it
    /// held; the holder repeats this call to finish it. No start succeeds until release.
    pub fn hold(&self, by: &str, why: &str) -> Result<Holding, ServerError> {
        let _locked = self.home.lock(Doing::Holding)?;
        self.home.allow_postgresql()?;
        let hold = self.home.hold_as(by, why)?;
        let stopped = self.stop_held()?;
        Ok(Holding {
            hold,
            stopped,
            data: self.data(),
        })
    }

    /// The board's hold, including an unreadable hold, which fails closed.
    pub fn held(&self) -> Option<Hold> {
        self.home.held()
    }

    /// Release without starting anything. Only the holder, or someone naming that holder, can.
    pub fn release(&self, by: &str, of: Option<&str>) -> Result<Option<Hold>, ServerError> {
        self.home.release(by, of)
    }

    /// Keep the complete cluster while stopped, under the starters' lock. Refuse a running
    /// server or an unreadable status. A clone is preferred, with a plain copy on other volumes.
    /// Snapshots are kept until explicitly removed; a partial copy is never listed as one.
    pub fn snapshot(&self, now: f64) -> Result<PathBuf, ServerError> {
        let _locked = self.home.lock(Doing::Snapshotting)?;
        self.snapshot_held(now)
    }

    /// [`Postgres::snapshot`] under the caller's starters' lock.
    pub(in crate::server) fn snapshot_held(&self, now: f64) -> Result<PathBuf, ServerError> {
        self.home.allow_postgresql()?;
        self.stopped()?;
        let folder = self.home.dir.join(SNAPSHOTS);
        fs::create_dir_all(&folder).map_err(io("could not make", &folder))?;
        let taking = folder.join(TAKING);
        copy_cluster(&self.data(), &taking)?;
        let base = stamp(now);
        let mut name = base.clone();
        let mut suffix = 0;
        while folder.join(&name).exists() {
            suffix += 1;
            name = format!("{base}.{suffix}");
        }
        let path = folder.join(name);
        fs::rename(&taking, &path).map_err(io("could not publish", &path))?;
        Ok(path)
    }

    /// Whole snapshots, newest names first. The work folder of an interrupted copy is excluded.
    pub fn snapshots(&self) -> Result<Vec<PathBuf>, ServerError> {
        let folder = self.home.dir.join(SNAPSHOTS);
        let entries = match fs::read_dir(&folder) {
            Ok(entries) => entries,
            Err(why) if why.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(why) => return Err(io("could not read", &folder)(why)),
        };
        let mut paths = Vec::new();
        for entry in entries {
            let entry = entry.map_err(io("could not read", &folder))?;
            if entry.file_name().to_str().is_some_and(named)
                && entry.path().join("PG_VERSION").is_file()
            {
                paths.push(entry.path());
            }
        }
        crate::order::sort_by(&mut paths, &|a, b| {
            let a = a.file_name().unwrap().to_str().unwrap();
            let b = b.file_name().unwrap().to_str().unwrap();
            order(b).cmp(&order(a))
        });
        Ok(paths)
    }

    /// Recover a snapshot into this new scratch board, without modifying or starting the source.
    /// Existing data is never replaced, and a failed or interrupted copy is retried from scratch.
    /// The board is left bound to no recovery generation, revoked before its data is published:
    /// a revoke that fails publishes nothing.
    pub fn restore(&self, snapshot: &Path) -> Result<(), ServerError> {
        let _locked = self.home.lock(Doing::Snapshotting)?;
        self.home.allow_postgresql()?;
        self.home.unheld()?;
        let data = self.data();
        if data.exists() {
            return Err(ServerError::Io(format!(
                "{} already holds PostgreSQL data: recovery needs a new scratch board",
                data.display()
            )));
        }
        let taking = self.staged(snapshot)?;
        // The copied cluster holds the recovery generation its source had. This home is bound to
        // none before that data is in place, so nothing written here takes a token read on the
        // source until a rebind replaces the generation (`crate::store::recovery`).
        self.home.revoke_recoveries()?;
        fs::rename(&taking, &data).map_err(io("could not publish", &data))
    }

    /// `snapshot` copied whole beside the board's data, as `postgresql.restoring`, ready to be
    /// moved in: refused for a cluster of another major, or one a server runs on.
    fn staged(&self, snapshot: &Path) -> Result<PathBuf, ServerError> {
        let version_file = snapshot.join("PG_VERSION");
        let version =
            fs::read_to_string(&version_file).map_err(io("could not read", &version_file))?;
        if version.trim().parse::<u32>().ok() != super::major(&self.version()?) {
            return Err(ServerError::Io(format!(
                "{} is PostgreSQL {}, and physical recovery needs the same major",
                snapshot.display(),
                version.trim()
            )));
        }
        self.stopped_at(snapshot)?;
        let taking = self.home.dir.join(format!("{DATA}.restoring"));
        copy_cluster(snapshot, &taking)?;
        // A snapshot taken after an unclean end can carry the old server's pid file. Its data is
        // now a different cluster path, whose new server must make its own record.
        let pid = taking.join("postmaster.pid");
        match fs::remove_file(&pid) {
            Ok(()) => {}
            Err(why) if why.kind() == std::io::ErrorKind::NotFound => {}
            Err(why) => return Err(io("could not remove", &pid)(why)),
        }
        Ok(taking)
    }

    /// `snapshot`, one of this board's own ([`Postgres::snapshots`]), put back over the board it
    /// was taken from, as `by`, at `now`: the folder that keeps the cluster it replaced. The
    /// board is held for it (the caller's hold, left standing, or one set now and released
    /// after), its server stopped, the snapshot copied into a work folder and given its leases
    /// past the replaced cluster's ([`Postgres::fenced`]), then switched in under
    /// the journal a move's switch runs under ([`crate::server::moving`]): the cluster in place
    /// moved aside into `moved/`, the copy moved in, the commit, the home's recovery bindings
    /// revoked. Nothing is deleted. A failure before the commit leaves the board on the cluster
    /// it had; a journal an earlier call left is settled instead, which is then what this
    /// answers (a committed restore finished) or refuses with (one undone).
    pub fn restore_over(
        &self,
        snapshot: &Path,
        by: &str,
        now: f64,
    ) -> Result<PathBuf, ServerError> {
        use crate::server::moving::{Journal, Kind, Left, Settled};
        let home = &self.home;
        let _locked = home.lock(Doing::Moving)?;
        match home.settle(by)? {
            Settled::Nothing => {}
            Settled::Finished(journal) => return Ok(journal.aside),
            Settled::Undone(journal) => {
                return Err(ServerError::Io(format!(
                    "a switch to {} had stopped before it committed, and is now undone: the \
                     board is on {} as it was. Nothing was restored by this command; run it again",
                    journal.to, journal.from
                )));
            }
        }
        home.allow_postgresql()?;
        let canonical = |path: &Path| path.canonicalize().ok();
        let own = self.snapshots()?;
        if canonical(snapshot).is_none()
            || !own.iter().any(|one| canonical(one) == canonical(snapshot))
        {
            return Err(ServerError::Io(format!(
                "{} is no snapshot of this board ({}): nothing was restored",
                snapshot.display(),
                home.dir.join(SNAPSHOTS).display()
            )));
        }
        if !self.data().exists() {
            return Err(ServerError::Io(format!(
                "the board has no PostgreSQL data at {} to restore over: a board with none is \
                 restored into with `Postgres::restore`",
                self.data().display()
            )));
        }
        let took = home.held_for(by, "restoring a snapshot over the board")?;
        let staging = home.dir.join(format!("{DATA}.restoring"));
        let work = home.dir.join(RESTORING);
        let left = |why: ServerError| {
            let _ = fs::remove_dir_all(&staging);
            let _ = fs::remove_dir_all(&work);
            let released = if took {
                home.release_as(by, None)
                    .map(|_| "and its hold is released")
            } else {
                Ok("and still held")
            };
            match released {
                Ok(hold) => ServerError::Io(format!(
                    "the snapshot was not restored: {why}. The board is on the cluster it had, \
                     {hold}"
                )),
                Err(unreleased) => unreleased,
            }
        };
        let staged = self
            .stop_held()
            .and_then(|_| self.stopped())
            .and_then(|()| self.fenced(snapshot, &work));
        let staged = match staged {
            Ok(staged) => staged,
            Err(why) => return Err(left(why)),
        };
        let engine = crate::server::Engine::PostgreSql;
        let aside = home.aside(now, engine);
        let journal = Journal::new(
            Kind::Restore,
            (engine, engine),
            &aside,
            &staged,
            &work,
            (by, took),
        );
        match home.switch(journal) {
            Ok(()) => {}
            Err(Left::Unchanged(why)) => return Err(left(why)),
            Err(Left::Standing(why)) => return Err(why),
        }
        if took {
            home.release_as(by, None)?;
        }
        Ok(aside)
    }

    /// `snapshot` copied into `work` as a cluster ready to be moved in, its leases ended and
    /// each revision past the one the cluster in place holds
    /// ([`crate::store::leases_past`]): the copy's data. The cluster in place is served once,
    /// by a server of the restore's own, to be read, and the copy once to be written; neither is
    /// left running. A cluster in place that cannot be served or read does not stop the restore,
    /// which is the way back from one that broke: the copy's leases are then moved on by more
    /// than any holder took, and the home is told.
    fn fenced(&self, snapshot: &Path, work: &Path) -> Result<PathBuf, ServerError> {
        use crate::store::{Opening, Store};
        let opened = |pg: &Postgres| {
            Store::unchecked_on(
                crate::pg::Client::new(&pg.socket(), crate::DATABASE).into(),
                Box::new(crate::store::now),
                Box::new(crate::sys::pid_alive),
                &crate::sys::hostname(),
                &Opening::write(),
            )
        };
        let read = self
            .serve()
            .and_then(|_| (opened(self).carried()).map_err(|why| ServerError::Io(why.to_string())));
        self.stop_held()?;
        self.stopped()?;
        let replaced = match read {
            Ok(replaced) => Some(replaced),
            Err(why) => {
                (self.home.tell)(&format!(
                    "· the cluster in place could not be read ({why}): the snapshot's leases \
                     are each moved on past any a holder took, not past the ones it holds"
                ));
                None
            }
        };
        let taking = self.staged(snapshot)?;
        fs::create_dir_all(work).map_err(io("could not make", work))?;
        let made = crate::server::Home {
            dir: work.into(),
            cut: None,
            ..self.home.clone()
        }
        .supervised(true);
        made.identify(crate::server::Engine::PostgreSql)?;
        let copy = made.postgresql(&self.programs);
        fs::rename(&taking, copy.data()).map_err(io("could not place", &copy.data()))?;
        copy.serve()?;
        let store = opened(&copy);
        let wrote = store.carried().and_then(|restored| {
            crate::store::leases_past(&restored, replaced.as_ref())
                .into_iter()
                .try_for_each(|mut lease| {
                    let id = lease["id"].take();
                    lease
                        .as_object_mut()
                        .expect("a lease is a row")
                        .remove("id");
                    store.raw().row("callback_lease", id).put(lease)
                })
        });
        copy.stop_held()?;
        wrote.map_err(|why| {
            ServerError::Io(format!("the snapshot's leases could not be ended: {why}"))
        })?;
        Ok(copy.data())
    }

    /// Reclaim the named database's dead space using PostgreSQL's rewrite. It needs temporary
    /// disk room and locks each table exclusively; it changes no board rows or schema version.
    pub fn reclaim(&self, database: &str) -> Result<(), ServerError> {
        assert!(crate::db::plain(database), "a database is a plain name");
        let _locked = self.home.lock(Doing::Reclaiming)?;
        self.home.allow_postgresql()?;
        self.home.unheld()?;
        if self.running(&self.version()?)?.is_none() {
            return Err(ServerError::NotUp(
                "PostgreSQL must be running to reclaim a database".into(),
            ));
        }
        crate::pg::Client::new(&self.socket(), database)
            .simple("VACUUM (FULL, ANALYZE)")
            .map(|_| ())
            .map_err(|why| {
                ServerError::Io(format!(
                    "could not reclaim PostgreSQL database {database}: {why:?}"
                ))
            })
    }

    fn stopped(&self) -> Result<(), ServerError> {
        self.stopped_at(&self.data())
    }

    fn stopped_at(&self, data: &Path) -> Result<(), ServerError> {
        let pg_ctl = self.program("pg_ctl");
        let said = output(server_command(&pg_ctl).args(["status", "-D"]).arg(data))
            .map_err(io("could not run", &pg_ctl))?;
        if said.status.code() == Some(3) {
            Ok(())
        } else {
            Err(ServerError::Io(format!(
                "PostgreSQL at {} is running or its status is unknown: no snapshot was taken ({})",
                data.display(),
                String::from_utf8_lossy(&said.stdout).trim()
            )))
        }
    }
}

/// Copy a whole private cluster, never a cluster with external tablespaces whose files would be
/// absent. Remove only the work folder this operation owns, including an interrupted attempt.
fn copy_cluster(from: &Path, to: &Path) -> Result<(), ServerError> {
    let source = from
        .canonicalize()
        .map_err(io("could not resolve snapshot source", from))?;
    let target = to
        .parent()
        .unwrap()
        .canonicalize()
        .map_err(io("could not resolve snapshot target", to))?
        .join(to.file_name().unwrap());
    if source.starts_with(&target) || target.starts_with(&source) {
        return Err(ServerError::Io(
            "snapshot source and target overlap: nothing was copied or removed".into(),
        ));
    }
    let tablespaces = from.join("pg_tblspc");
    let mut entries = fs::read_dir(&tablespaces).map_err(io("could not read", &tablespaces))?;
    if entries.next().is_some() {
        return Err(ServerError::Io(
            "a board snapshot cannot carry external PostgreSQL tablespaces".into(),
        ));
    }
    if to.exists() {
        fs::remove_dir_all(to).map_err(io("could not remove partial copy", to))?;
    }
    if let Err(why) = clone(from, to) {
        let _ = fs::remove_dir_all(to);
        copy(from, to).map_err(|again| {
            ServerError::Io(format!(
                "could not clone ({why}) or copy ({again}) the PostgreSQL cluster"
            ))
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_complete_named_snapshots_are_listed_in_numeric_order_within_a_second() {
        let dir = std::env::temp_dir().join(format!("pg-snapshot-list-{}", std::process::id()));
        let home = crate::server::Home::new(&dir);
        let pg = home.postgresql(Path::new("/no/postgres"));
        let folder = dir.join(SNAPSHOTS);
        let base = "1970-01-01T00-00-01Z";
        for name in (0..12)
            .map(|n| {
                if n == 0 {
                    base.into()
                } else {
                    format!("{base}.{n}")
                }
            })
            .chain([TAKING.into(), "not-a-snapshot".into()])
        {
            let path = folder.join(name);
            fs::create_dir_all(&path).unwrap();
            fs::write(path.join("PG_VERSION"), "18").unwrap();
        }
        fs::create_dir(folder.join(format!("{base}.50"))).unwrap();
        let found = pg.snapshots().unwrap();
        assert_eq!(found.len(), 12);
        assert_eq!(found[0].file_name().unwrap(), format!("{base}.11").as_str());
        assert_eq!(found[1].file_name().unwrap(), format!("{base}.10").as_str());
        assert_eq!(found.last().unwrap().file_name().unwrap(), base);
        fs::remove_dir_all(dir).unwrap();
    }
}
