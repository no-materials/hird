// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Child-process timings: `erlc` and the emulator over what `hird build`
//! writes, and end-to-end `hird` subcommands with their emulator boots.

use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// `name` (plus the platform's executable suffix) on `PATH`.
pub(crate) fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|dir| dir.join(format!("{name}{}", env::consts::EXE_SUFFIX)))
        .find(|candidate| candidate.is_file())
}

/// Runs `command` to completion with stdout discarded, returning its wall
/// time; a nonzero exit is an error carrying its stderr.
pub(crate) fn time(command: &mut Command) -> Result<Duration, String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let start = Instant::now();
    let output = command
        .output()
        .map_err(|e| format!("cannot run {command:?}: {e}"))?;
    let elapsed = start.elapsed();
    if !output.status.success() {
        return Err(format!(
            "{command:?} failed with {}:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(elapsed)
}

/// The BEAM stages' inputs: the build directory `hird build` wrote and the
/// `.erl` files in it.
#[derive(Debug)]
pub(crate) struct BuildDir {
    /// The directory.
    dir: PathBuf,
    /// Every `.erl` file in it, sorted: generated, runtime, and boot.
    pub(crate) erl_files: Vec<PathBuf>,
}

impl BuildDir {
    /// Builds `src` into `dir` with `hird build`, once and untimed.
    pub(crate) fn build(hird: &Path, src: &Path, dir: &Path) -> Result<Self, String> {
        time(Command::new(hird).arg("build").arg(src).arg("-o").arg(dir))?;
        let mut erl_files: Vec<PathBuf> = fs::read_dir(dir)
            .map_err(|e| format!("cannot read `{}`: {e}", dir.display()))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "erl"))
            .collect();
        erl_files.sort();
        Ok(Self {
            dir: dir.to_path_buf(),
            erl_files,
        })
    }

    /// `erlc -o <dir> <every .erl>`: the compile `hird build` runs.
    pub(crate) fn erlc(&self) -> Result<Duration, String> {
        time(
            Command::new("erlc")
                .arg("-o")
                .arg(&self.dir)
                .args(&self.erl_files),
        )
    }

    /// `erl` booting the built program, running `main`, and halting: what
    /// `hird run` does after building.
    pub(crate) fn boot(&self) -> Result<Duration, String> {
        time(
            Command::new("erl")
                .args(["+Bi", "-noshell", "-pa"])
                .arg(&self.dir)
                .args(["-s", "hird_boot", "run"]),
        )
    }
}

/// Emulators one command started.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Boots {
    /// `erlc` invocations (one emulator each).
    pub(crate) erlc: usize,
    /// `erl` invocations.
    pub(crate) erl: usize,
}

/// Counts emulator boots: `erl` and `erlc` shims first on `PATH`, each
/// logging its name before handing over to the real binary.
#[derive(Debug)]
pub(crate) struct BootCounter {
    /// The log the shims append to.
    log: PathBuf,
    /// `PATH` with the shim directory first.
    path: OsString,
}

impl BootCounter {
    /// Installs the shims under `dir`; `None` where shims are not supported
    /// (non-Unix) or Erlang is not on `PATH`.
    #[cfg(unix)]
    pub(crate) fn install(dir: &Path) -> Result<Option<Self>, String> {
        use std::os::unix::fs::PermissionsExt as _;

        let (Some(erl), Some(erlc)) = (find_on_path("erl"), find_on_path("erlc")) else {
            return Ok(None);
        };
        fs::create_dir_all(dir).map_err(|e| format!("cannot create `{}`: {e}", dir.display()))?;
        let log = dir.join("boots.log");
        for (name, real) in [("erl", erl), ("erlc", erlc)] {
            let shim = dir.join(name);
            let script = format!(
                "#!/bin/sh\necho {name} >> {}\nexec {} \"$@\"\n",
                sh_quote(&log),
                sh_quote(&real)
            );
            fs::write(&shim, script)
                .and_then(|()| fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)))
                .map_err(|e| format!("cannot write `{}`: {e}", shim.display()))?;
        }
        let mut dirs = vec![dir.to_path_buf()];
        dirs.extend(env::split_paths(&env::var_os("PATH").unwrap_or_default()));
        let path = env::join_paths(dirs).map_err(|e| format!("cannot extend PATH: {e}"))?;
        Ok(Some(Self { log, path }))
    }

    /// Always `None`: the shims are shell scripts.
    #[cfg(not(unix))]
    pub(crate) fn install(_dir: &Path) -> Result<Option<Self>, String> {
        Ok(None)
    }

    /// Runs `command` under the shims, returning the boots it made.
    pub(crate) fn count(&self, command: &mut Command) -> Result<Boots, String> {
        fs::write(&self.log, "").map_err(|e| format!("cannot reset the boot log: {e}"))?;
        time(command.env("PATH", &self.path))?;
        let log =
            fs::read_to_string(&self.log).map_err(|e| format!("cannot read the boot log: {e}"))?;
        Ok(Boots {
            erlc: log.lines().filter(|l| *l == "erlc").count(),
            erl: log.lines().filter(|l| *l == "erl").count(),
        })
    }
}

/// `path` single-quoted for `sh`.
#[cfg(unix)]
fn sh_quote(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', r"'\''"))
}
