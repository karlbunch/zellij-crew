//! Per-session state under zellij's own tmp dir: the tell counter, the tell log,
//! and per-pane status files. Only the counter needs a lock.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use zellij_utils::consts::{ZELLIJ_TMP_DIR, ZELLIJ_TMP_LOG_DIR};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: u64,
    pub ts: u64,
    pub from: String,
    pub to: String,
    pub pane: u32,
    pub msg: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Status {
    pub pane: u32,
    pub state: String,
    pub ts: u64,
}

pub struct State {
    dir: PathBuf,
}

impl State {
    pub fn open(session: &str) -> Result<Self> {
        let dir = ZELLIJ_TMP_DIR.join("zellij-crew").join(session);
        fs::create_dir_all(dir.join("status"))
            .with_context(|| format!("creating {}", dir.display()))?;
        Ok(Self { dir })
    }

    /// Hand out the next message id under an exclusive lock, so two concurrent
    /// tells never share one. The lock is released when the file handle drops.
    pub fn next_msg_id(&self) -> Result<u64> {
        let lock = OpenOptions::new()
            .create(true)
            .write(true)
            .open(self.dir.join("lock"))?;
        // SAFETY: flock on a file descriptor we own; the fd stays open until `lock` drops.
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } != 0 {
            return Err(std::io::Error::last_os_error()).context("locking the message counter");
        }
        let path = self.dir.join("next_msg_id");
        let id: u64 = fs::read_to_string(&path)
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(1);
        fs::write(&path, (id + 1).to_string())?;
        Ok(id)
    }

    pub fn log_message(&self, m: &Message) -> Result<()> {
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir.join("messages.jsonl"))?;
        writeln!(f, "{}", serde_json::to_string(m)?)?;
        Ok(())
    }

    /// Every logged message, oldest first. Unparseable lines are skipped.
    pub fn messages(&self) -> Vec<Message> {
        let Ok(f) = File::open(self.dir.join("messages.jsonl")) else { return vec![] };
        BufReader::new(f)
            .lines()
            .map_while(Result::ok)
            .filter_map(|l| serde_json::from_str(&l).ok())
            .collect()
    }

    pub fn set_status(&self, pane: u32, state: &str) -> Result<()> {
        let s = Status { pane, state: state.to_owned(), ts: now() };
        fs::write(self.dir.join("status").join(format!("{pane}.json")), serde_json::to_string(&s)?)?;
        Ok(())
    }

    pub fn status(&self, pane: u32) -> Option<Status> {
        let text = fs::read_to_string(self.dir.join("status").join(format!("{pane}.json"))).ok()?;
        serde_json::from_str(&text).ok()
    }
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Best-effort diagnostics next to zellij.log.
pub fn log_line(line: &str) {
    let _ = fs::create_dir_all(&*ZELLIJ_TMP_LOG_DIR);
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(ZELLIJ_TMP_LOG_DIR.join("zellij-crew.log"))
    {
        let _ = writeln!(f, "{} {}", now(), line);
    }
}
