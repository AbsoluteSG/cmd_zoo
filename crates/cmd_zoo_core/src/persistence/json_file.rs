use std::fs::{self, File, OpenOptions};
use std::path::PathBuf;
use std::time::SystemTime;

use anyhow::{Context, Result, anyhow};
use directories::ProjectDirs;
use fs2::FileExt;

use super::{ZooRepository, snapshot_from_zoo, zoo_from_snapshot};
use crate::game::Zoo;

pub struct JsonFileRepository {
    path: PathBuf,
}

impl JsonFileRepository {
    pub fn default_path() -> Result<PathBuf> {
        let pd = ProjectDirs::from("", "", "cmd_zoo")
            .ok_or_else(|| anyhow!("could not determine user data directory"))?;
        Ok(pd.data_dir().join("save.json"))
    }

    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn at_default_path() -> Result<Self> {
        Ok(Self::new(Self::default_path()?))
    }

    fn lock_path(&self) -> PathBuf {
        self.path.with_extension("json.lock")
    }

    fn ensure_parent(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating data dir {}", parent.display()))?;
        }
        Ok(())
    }

    /// Acquire an exclusive cross-process lock on the save file. Blocks until
    /// the lock is available. Returns a guard whose Drop releases the lock —
    /// callers should hold it across the load/mutate/save critical section so
    /// concurrent instances see a consistent view.
    ///
    /// The lock is held on a sidecar `save.json.lock` (never written to)
    /// rather than on `save.json` itself, because our save path is
    /// write-temp-then-rename — holding a Windows lock on the destination
    /// file would prevent the rename.
    pub fn lock(&self) -> Result<LockedAccess<'_>> {
        self.ensure_parent()?;
        let lock_path = self.lock_path();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .with_context(|| format!("opening lock file {}", lock_path.display()))?;
        file.lock_exclusive()
            .with_context(|| format!("acquiring lock on {}", lock_path.display()))?;
        Ok(LockedAccess {
            repo: self,
            _file: file,
        })
    }
}

/// RAII guard holding the cross-process save-file lock. While alive, this
/// instance has exclusive access — `load_if_newer` and `save` can be called
/// in any order. Drop releases the lock.
pub struct LockedAccess<'a> {
    repo: &'a JsonFileRepository,
    /// Holds the OS-level file lock; released on Drop via fs2's
    /// `Drop for File` -> unlock-on-close behavior (we also unlock explicitly).
    _file: File,
}

impl<'a> LockedAccess<'a> {
    /// Reload `Zoo` from disk if the file's modtime is newer than `since`.
    /// `since == SystemTime::UNIX_EPOCH` forces a read even if the file is
    /// older than our process start (used on initial load).
    ///
    /// The third tuple element is a list of human-readable warnings about
    /// entries dropped from the snapshot (unknown species ids, retired
    /// structure kinds). Always empty for clean saves; non-empty when the
    /// catalog has changed shape between the save and the current binary.
    pub fn load_if_newer(
        &self,
        since: SystemTime,
    ) -> Result<Option<(Zoo, SystemTime, Vec<String>)>> {
        if !self.repo.path.exists() {
            return Ok(None);
        }
        let modtime = fs::metadata(&self.repo.path)
            .with_context(|| format!("stat save file {}", self.repo.path.display()))?
            .modified()
            .context("filesystem does not report modtime")?;
        if modtime <= since {
            return Ok(None);
        }
        let bytes = fs::read(&self.repo.path)
            .with_context(|| format!("reading save file {}", self.repo.path.display()))?;
        let (snap, notes) =
            super::parse_snapshot_with_notes(&bytes).context("parsing save file")?;
        let mut loaded = zoo_from_snapshot(snap)?;
        // Prepend migration notes (e.g. v8→v9 consolidation summary) so the
        // user sees them in the status bar alongside any species drops.
        for m in notes.messages {
            loaded.warnings.push(m);
        }
        Ok(Some((loaded.zoo, modtime, loaded.warnings)))
    }

    /// Atomically write `zoo` to disk and return the new file modtime.
    pub fn save(&self, zoo: &Zoo) -> Result<SystemTime> {
        let snap = snapshot_from_zoo(zoo);
        let bytes = serde_json::to_vec_pretty(&snap)?;
        let tmp = self.repo.path.with_extension("json.tmp");
        fs::write(&tmp, &bytes)
            .with_context(|| format!("writing temp save {}", tmp.display()))?;
        fs::rename(&tmp, &self.repo.path).with_context(|| {
            format!("atomically replacing {}", self.repo.path.display())
        })?;
        let mt = fs::metadata(&self.repo.path)
            .with_context(|| format!("stat save file {}", self.repo.path.display()))?
            .modified()
            .context("filesystem does not report modtime")?;
        Ok(mt)
    }
}

impl<'a> Drop for LockedAccess<'a> {
    fn drop(&mut self) {
        // fs2's File Drop already unlocks, but be explicit to make the
        // contract obvious to readers and to catch errors during debugging.
        let _ = FileExt::unlock(&self._file);
    }
}

impl ZooRepository for JsonFileRepository {
    /// Single-instance convenience: lock, read, unlock. Warnings are
    /// discarded here — the caller (tests or future backends) gets just the
    /// `Zoo`. The shared-instance fast path uses `lock().load_if_newer()`
    /// directly and surfaces the warning list to the user.
    fn load(&self) -> Result<Option<Zoo>> {
        let access = self.lock()?;
        Ok(access
            .load_if_newer(SystemTime::UNIX_EPOCH)?
            .map(|(zoo, _, _)| zoo))
    }

    /// Single-instance convenience: lock, write, unlock.
    fn save(&self, zoo: &Zoo) -> Result<()> {
        let access = self.lock()?;
        access.save(zoo)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::Zoo;
    use chrono::Utc;
    use tempfile::tempdir;

    #[test]
    fn load_if_newer_returns_none_when_unchanged() {
        let dir = tempdir().unwrap();
        let repo = JsonFileRepository::new(dir.path().join("save.json"));
        let now = Utc::now();
        let zoo = Zoo::new(now);
        let mtime = {
            let g = repo.lock().unwrap();
            g.save(&zoo).unwrap()
        };
        // Second call with the just-recorded modtime should report no change.
        let g = repo.lock().unwrap();
        let res = g.load_if_newer(mtime).unwrap();
        assert!(res.is_none(), "expected no reload when modtime unchanged");
    }

    #[test]
    fn save_then_load_if_newer_returns_zoo() {
        let dir = tempdir().unwrap();
        let repo = JsonFileRepository::new(dir.path().join("save.json"));
        let now = Utc::now();
        let mut zoo = Zoo::new(now);
        zoo.coins = 1234;
        {
            let g = repo.lock().unwrap();
            g.save(&zoo).unwrap();
        }
        // Use UNIX_EPOCH as "since" → forces a read.
        let g = repo.lock().unwrap();
        let (loaded, _mtime, _warnings) =
            g.load_if_newer(SystemTime::UNIX_EPOCH).unwrap().unwrap();
        assert_eq!(loaded.coins, 1234);
    }

    /// Simulates the multi-instance flow end-to-end: process A saves an
    /// initial zoo; process B (us) loads it; process A mutates and resaves
    /// behind B's back; B's next `load_if_newer` picks up the change.
    #[test]
    fn external_mutation_is_picked_up_by_load_if_newer() {
        let dir = tempdir().unwrap();
        let repo = JsonFileRepository::new(dir.path().join("save.json"));
        let now = Utc::now();

        // Process A: initial save.
        let mut zoo_a = Zoo::new(now);
        zoo_a.coins = 100;
        let mtime_after_initial = {
            let g = repo.lock().unwrap();
            g.save(&zoo_a).unwrap()
        };

        // Process B loads at startup.
        let (mut zoo_b, mut known_mtime, _warnings) = {
            let g = repo.lock().unwrap();
            g.load_if_newer(SystemTime::UNIX_EPOCH).unwrap().unwrap()
        };
        assert_eq!(zoo_b.coins, 100);
        assert_eq!(known_mtime, mtime_after_initial);

        // Sleep a tick so the filesystem records a strictly newer modtime
        // (FAT, exFAT and some VMs only have second-precision modtime).
        std::thread::sleep(std::time::Duration::from_millis(1100));

        // Process A mutates and resaves.
        zoo_a.coins = 250;
        let new_mtime = {
            let g = repo.lock().unwrap();
            g.save(&zoo_a).unwrap()
        };
        assert!(new_mtime > mtime_after_initial);

        // Process B's next tick sees the change.
        {
            let g = repo.lock().unwrap();
            let res = g.load_if_newer(known_mtime).unwrap();
            assert!(res.is_some(), "load_if_newer should report the external write");
            let (loaded, mt, _warnings) = res.unwrap();
            zoo_b = loaded;
            known_mtime = mt;
        }
        assert_eq!(zoo_b.coins, 250);
        assert_eq!(known_mtime, new_mtime);
    }

    /// Two threads each writing N times should fully serialize. Each save
    /// either lands intact or is overwritten by a later one — no torn JSON.
    #[test]
    fn concurrent_writes_are_serialized() {
        use std::sync::Arc;
        let dir = tempdir().unwrap();
        let path = dir.path().join("save.json");
        let repo = Arc::new(JsonFileRepository::new(path.clone()));
        let now = Utc::now();
        let mut z1 = Zoo::new(now);
        z1.coins = 11;
        let mut z2 = Zoo::new(now);
        z2.coins = 22;
        let r1 = Arc::clone(&repo);
        let r2 = Arc::clone(&repo);
        let t1 = std::thread::spawn(move || {
            for _ in 0..20 {
                let g = r1.lock().unwrap();
                g.save(&z1).unwrap();
            }
        });
        let t2 = std::thread::spawn(move || {
            for _ in 0..20 {
                let g = r2.lock().unwrap();
                g.save(&z2).unwrap();
            }
        });
        t1.join().unwrap();
        t2.join().unwrap();
        // Whatever landed last must parse back as a valid Zoo (no torn write).
        let final_zoo = repo.load().unwrap().unwrap();
        assert!(final_zoo.coins == 11 || final_zoo.coins == 22);
    }
}
