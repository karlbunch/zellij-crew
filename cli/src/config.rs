//! `zellij-crew.kdl`, found next to zellij's own `config.kdl`.

use anyhow::{Context, Result};
use kdl::KdlDocument;
use std::path::{Path, PathBuf};
use zellij_utils::consts::ZELLIJ_CONFIG_DIR_ENV;
use zellij_utils::home::find_default_config_dir;

pub const DEFAULT_PREFIX: &str = "[CREW MESSAGE #{id} from {from}; to: {to}] ";
pub const DEFAULT_POSTFIX: &str = "*CRITICAL* Reply ONLY by running this bash command, \
    do not just output your response: zellij-crew tell {from} \"your reply here\"";
pub const DEFAULT_ENTER_DELAY_MS: u64 = 250;

pub struct Config {
    pub prefix: String,
    pub postfix: String,
    pub enter_delay_ms: u64,
    /// Where the config was read from, if a file existed.
    pub path: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            prefix: DEFAULT_PREFIX.to_owned(),
            postfix: DEFAULT_POSTFIX.to_owned(),
            enter_delay_ms: DEFAULT_ENTER_DELAY_MS,
            path: None,
        }
    }
}

impl Config {
    /// The config path: explicit, else `$ZELLIJ_CONFIG_DIR`, else zellij's default dir.
    pub fn path(explicit: Option<&Path>) -> Option<PathBuf> {
        if let Some(p) = explicit {
            return Some(p.to_path_buf());
        }
        let dir = std::env::var_os(ZELLIJ_CONFIG_DIR_ENV)
            .map(PathBuf::from)
            .or_else(find_default_config_dir)?;
        Some(dir.join("zellij-crew.kdl"))
    }

    /// Built-in defaults, overridden by whatever the file sets. A missing default
    /// file is fine; a missing explicit one is an error.
    pub fn load(explicit: Option<&Path>) -> Result<Self> {
        let mut cfg = Self::default();
        let Some(path) = Self::path(explicit) else { return Ok(cfg) };
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && explicit.is_none() => {
                return Ok(cfg)
            },
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        let doc: KdlDocument = text
            .parse()
            .with_context(|| format!("parsing {}", path.display()))?;
        if let Some(tell) = doc.get("tell").and_then(|n| n.children()) {
            if let Some(v) = tell.get_arg("prefix").and_then(|v| v.as_string()) {
                cfg.prefix = v.to_owned();
            }
            if let Some(v) = tell.get_arg("postfix").and_then(|v| v.as_string()) {
                cfg.postfix = v.to_owned();
            }
            if let Some(v) = tell.get_arg("enter_delay_ms").and_then(|v| v.as_i64()) {
                cfg.enter_delay_ms = v.max(0) as u64;
            }
        }
        cfg.path = Some(path);
        Ok(cfg)
    }
}
