// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The build cache: compiled `.beam` files addressed by a hash of their
//! Erlang source, the Erlang install and build driver that compiled them,
//! and the compiler environment, shared by every build on the machine.

use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// Bumped when an entry's meaning changes, orphaning every older entry.
const FORMAT: &str = "hird beam cache 2";

/// Environment variables that change what the Erlang compiler produces.
const COMPILER_ENV: [&str; 1] = ["ERL_COMPILER_OPTIONS"];

/// The cache directory, its compiler fingerprint folded into a hash prefix.
#[derive(Debug)]
pub(crate) struct BeamCache {
    /// Where entries live: `<root>/beam`.
    dir: PathBuf,
    /// The hash state after the format, `erl` fingerprint, build driver,
    /// and compiler environment; each key continues it with a module's
    /// source.
    prefix: Fnv128,
}

impl BeamCache {
    /// The cache for modules compiled in the `erl` on `PATH` by the build
    /// driver whose Erlang source is `driver` (it fixes the compile
    /// options), created if missing. `None` when no cache directory or
    /// `erl` can be found; a directory that cannot be created is warned
    /// about.
    pub(crate) fn open(driver: &str) -> Option<Self> {
        let erl = fingerprint(&find_on_path("erl")?)?;
        let dir = root()?.join("beam");
        if let Err(e) = fs::create_dir_all(&dir) {
            eprintln!(
                "hird: warning: build cache off: cannot create `{}`: {e}",
                dir.display()
            );
            return None;
        }
        let mut prefix = Fnv128::new();
        prefix.field(FORMAT.as_bytes());
        prefix.field(&erl);
        prefix.field(driver.as_bytes());
        for var in COMPILER_ENV {
            prefix.field(env::var_os(var).unwrap_or_default().as_encoded_bytes());
        }
        Some(Self { dir, prefix })
    }

    /// The entry name for a module whose Erlang source is `source`.
    pub(crate) fn key(&self, source: &str) -> String {
        let mut hash = self.prefix.clone();
        hash.field(source.as_bytes());
        format!("{:032x}", hash.0)
    }

    /// Copies entry `key` to `beam`; whether the cache held it.
    pub(crate) fn fetch(&self, key: &str, beam: &Path) -> bool {
        fs::copy(self.entry(key), beam).is_ok()
    }

    /// Where entries live. The build driver stores each module it compiles
    /// here as `<key>.beam`, written aside and renamed into place.
    pub(crate) fn dir(&self) -> &Path {
        &self.dir
    }

    /// The path of entry `key`.
    fn entry(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.beam"))
    }
}

/// The cache root: `HIRD_CACHE_DIR`, else `hird` under the platform's user
/// cache directory.
fn root() -> Option<PathBuf> {
    let var = |name: &str| {
        env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    var("HIRD_CACHE_DIR")
        .or_else(|| var("XDG_CACHE_HOME").map(|dir| dir.join("hird")))
        .or_else(|| var("LOCALAPPDATA").map(|dir| dir.join("hird")))
        .or_else(|| var("HOME").map(|dir| dir.join(".cache").join("hird")))
}

/// `name` (plus the platform's executable suffix) on `PATH`, as
/// `Command::new(name)` would find it.
fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|dir| dir.join(format!("{name}{}", env::consts::EXE_SUFFIX)))
        .find(|candidate| candidate.is_file())
}

/// What identifies the Erlang install behind `erl` without booting it:
/// the resolved executable's path, size, and modification time.
fn fingerprint(erl: &Path) -> Option<Vec<u8>> {
    let resolved = fs::canonicalize(erl).ok()?;
    let meta = fs::metadata(&resolved).ok()?;
    let mtime = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    let mut out = OsString::from(resolved).into_encoded_bytes();
    out.extend_from_slice(&meta.len().to_le_bytes());
    out.extend_from_slice(&mtime.as_nanos().to_le_bytes());
    Some(out)
}

/// 128-bit FNV-1a: stable across builds and platforms, wide enough that
/// distinct sources do not collide.
#[derive(Clone, Debug)]
struct Fnv128(u128);

impl Fnv128 {
    /// The FNV-1a 128-bit offset basis.
    fn new() -> Self {
        Self(0x6c62_272e_07bb_0142_62b8_2175_6295_c58d)
    }

    /// Hashes `bytes`.
    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 ^= u128::from(byte);
            self.0 = self
                .0
                .wrapping_mul(0x0000_0000_0100_0000_0000_0000_0000_013b);
        }
    }

    /// Hashes `bytes` length-prefixed, so field boundaries cannot shift.
    fn field(&mut self, bytes: &[u8]) {
        self.write(&(bytes.len() as u64).to_le_bytes());
        self.write(bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cache in `dir` with a fixed fingerprint and build driver.
    fn cache(dir: &Path, erl: &[u8], driver: &str) -> BeamCache {
        let mut prefix = Fnv128::new();
        prefix.field(FORMAT.as_bytes());
        prefix.field(erl);
        prefix.field(driver.as_bytes());
        BeamCache {
            dir: dir.to_path_buf(),
            prefix,
        }
    }

    #[test]
    fn fnv128_matches_the_reference_vectors() {
        for (input, expected) in [
            ("", 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d),
            ("a", 0xd228_cb69_6f1a_8caf_7891_2b70_4e4a_8964),
            ("foobar", 0x343e_1662_793c_64bf_6f0d_3597_ba44_6f18),
        ] {
            let mut hash = Fnv128::new();
            hash.write(input.as_bytes());
            assert_eq!(hash.0, expected, "{input:?}");
        }
    }

    #[test]
    fn keys_follow_source_compiler_and_driver() {
        let dir = Path::new("unused");
        let a = cache(dir, b"/otp-27/bin/erl", "-module(hird_build).");
        assert_eq!(a.key("-module(m)."), a.key("-module(m)."));
        assert_ne!(a.key("-module(m)."), a.key("-module(n)."));
        let b = cache(dir, b"/otp-28/bin/erl", "-module(hird_build).");
        assert_ne!(a.key("-module(m)."), b.key("-module(m)."));
        let c = cache(dir, b"/otp-27/bin/erl", "-module(hird_build). %% edited");
        assert_ne!(a.key("-module(m)."), c.key("-module(m)."));
    }
}
