// Copyright 2026 the Hird Authors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The no-argument record/replay demonstration: record one run of the
//! embedded agent planner, then replay that one recording against variants
//! of the program and report where each variant's decisions part from it.

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::{Failure, fail};

/// The demo program, embedded so the subcommand needs no source tree and
/// no arguments.
const DEMO_SOURCE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../demo/agent_planner.hird"
));

/// The file name every arm's source is written under: the module name is
/// derived from it, so the arms share the name and differ by directory.
const SOURCE_NAME: &str = "agent_planner.hird";

/// The recorded episode, written once and replayed by every arm.
const RECORDING_NAME: &str = "recording.jsonl";

/// The width the arm name is padded to in both tables.
const NAME_WIDTH: usize = 14;

/// One arm of the fan-out: the demo source under one edit.
struct Variant {
    /// The arm's name, in both tables.
    name: &'static str,
    /// What the edit changes, as its line of the first table.
    summary: &'static str,
    /// The edit as (text found in the demo source, replacement); `None`
    /// for the baseline, which is the demo verbatim.
    edit: Option<(&'static str, &'static str)>,
}

/// The fan-out's arms. The baseline comes first: it is the program the
/// recording is made from, so it replays green and every departure below
/// it belongs to an edit rather than to the demonstration.
const VARIANTS: [Variant; 3] = [
    Variant {
        name: "baseline",
        summary: "the demo, unedited",
        edit: None,
    },
    Variant {
        name: "announce-first",
        summary: "logs each ticket before filing it, not after",
        edit: Some((
            "      let _ = create_ticket({ title: title, body: body }) in\n\
             \x20     log({ level: \"info\", message: title });\n",
            "      log({ level: \"info\", message: title });\n\
             \x20     let _ = create_ticket({ title: title, body: body }) in\n",
        )),
    },
    Variant {
        name: "eager",
        summary: "files tickets for priority-0 tasks too",
        edit: Some(("if priority > 0", "if priority >= 0")),
    },
];

impl Variant {
    /// The arm's source: the demo with its edit applied.
    fn source(&self) -> Result<String, Failure> {
        let Some((from, to)) = self.edit else {
            return Ok(DEMO_SOURCE.to_owned());
        };
        let edited = DEMO_SOURCE.replace(from, to);
        if edited == DEMO_SOURCE {
            return Err(fail!(
                "the `{}` edit no longer applies to the demo source",
                self.name
            ));
        }
        Ok(edited)
    }

    /// Writes the arm's source into its own directory under `out_dir`,
    /// returning where the source and the build output go.
    fn write(&self, out_dir: &Path) -> Result<Arm, Failure> {
        let dir = out_dir.join(self.name);
        fs::create_dir_all(&dir).map_err(|e| fail!("cannot create `{}`: {e}", dir.display()))?;
        let source = dir.join(SOURCE_NAME);
        fs::write(&source, self.source()?)
            .map_err(|e| fail!("cannot write `{}`: {e}", source.display()))?;
        Ok(Arm {
            source,
            out_dir: dir.join("out"),
        })
    }
}

/// One arm on disk.
struct Arm {
    /// The written `.hird` source.
    source: PathBuf,
    /// The directory the arm's Erlang and `.beam` files are built into.
    out_dir: PathBuf,
}

/// What one arm made of the shared recording.
enum Outcome {
    /// The arm replayed the episode to its end: the same calls, in order.
    Agreed,
    /// The arm parted from the episode.
    Parted {
        /// The 0-based position of the first differing call, as the
        /// report names it.
        position: String,
        /// The divergence kind the run crashed with.
        kind: String,
    },
}

/// Records one run of the demo, replays that recording against every
/// variant, and prints the divergence table. Everything is written under
/// `out_dir`; nothing else on the machine is touched, and no service is
/// contacted.
pub(crate) fn run(out_dir: &Path) -> Result<(), Failure> {
    let hird =
        std::env::current_exe().map_err(|e| fail!("cannot locate the running hird binary: {e}"))?;
    fs::create_dir_all(out_dir).map_err(|e| fail!("cannot create `{}`: {e}", out_dir.display()))?;
    let recording = out_dir.join(RECORDING_NAME);
    // The audit sink appends, so an earlier run's recording would be
    // replayed along with this one's.
    if recording.exists() {
        fs::remove_file(&recording)
            .map_err(|e| fail!("cannot remove `{}`: {e}", recording.display()))?;
    }
    let arms: Vec<Arm> = VARIANTS
        .iter()
        .map(|variant| variant.write(out_dir))
        .collect::<Result<_, Failure>>()?;

    println!(
        "Hirð record/replay demo. Two commands, one program, no service contacted:\n\
         \n\
         \x20 hird run {SOURCE_NAME} --audit-file {RECORDING_NAME}   (record)\n\
         \x20 hird run {SOURCE_NAME} --replay {RECORDING_NAME}       (replay)\n"
    );

    // The episode is recorded from the first arm, the demo verbatim.
    let calls = record(&hird, &arms[0], &recording)?;
    println!("Recorded {calls} tool calls from one run of the supervised agent planner.\n");
    println!("Replaying that one recording against three variants of the program:\n");
    for variant in &VARIANTS {
        println!("  {:<NAME_WIDTH$}  {}", variant.name, variant.summary);
    }
    println!();
    for (variant, arm) in VARIANTS.iter().zip(&arms) {
        let outcome = replay(&hird, variant.name, arm, &recording)?;
        println!(
            "  {:<NAME_WIDTH$}  {}",
            variant.name,
            verdict(&outcome, calls)
        );
    }

    println!(
        "\n\
         Replay served every tool result from the recording, so all three arms\n\
         met a byte-identical environment: what separates them above is the\n\
         program, not the world. No handler ran and nothing was mocked — the\n\
         replay cursor outranks every install block.\n\
         \n\
         The sources, the recording, and the generated Erlang are in {}.",
        out_dir.display()
    );
    Ok(())
}

/// Records one run of `arm` into `recording`, returning the number of tool
/// calls the episode holds.
fn record(hird: &Path, arm: &Arm, recording: &Path) -> Result<usize, Failure> {
    let output = spawn(
        hird,
        arm,
        &[OsStr::new("--audit-file"), recording.as_os_str()],
    )?;
    if !output.status.success() {
        return Err(fail!(
            "recording the demo failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let episode = fs::read_to_string(recording)
        .map_err(|e| fail!("cannot read `{}`: {e}", recording.display()))?;
    Ok(episode.lines().count())
}

/// Replays `recording` against `arm`, reporting where the arm's decisions
/// leave the recorded episode.
fn replay(hird: &Path, name: &str, arm: &Arm, recording: &Path) -> Result<Outcome, Failure> {
    let output = spawn(hird, arm, &[OsStr::new("--replay"), recording.as_os_str()])?;
    if output.status.success() {
        return Ok(Outcome::Agreed);
    }
    let crash = String::from_utf8_lossy(&output.stderr);
    let (Some(position), Some(kind)) =
        (crash_field(&crash, "position"), crash_field(&crash, "kind"))
    else {
        return Err(fail!(
            "the `{name}` arm failed without a replay divergence:\n{crash}"
        ));
    };
    Ok(Outcome::Parted {
        position: position.to_owned(),
        kind: kind.to_owned(),
    })
}

/// Runs `hird run <source> -o <out_dir>` plus `extra` on one arm, capturing
/// its output: the audit stream, the Erlang compiler's warnings, and a
/// divergence crash report all belong to the demonstration's machinery,
/// not to its output.
fn spawn(hird: &Path, arm: &Arm, extra: &[&OsStr]) -> Result<Output, Failure> {
    Command::new(hird)
        .arg("run")
        .arg(&arm.source)
        .arg("-o")
        .arg(&arm.out_dir)
        .args(extra)
        .output()
        .map_err(|e| fail!("cannot run `{}`: {e}", hird.display()))
}

/// The value of `key => …` in an Erlang crash report, read to the first
/// character that ends an integer or an atom.
fn crash_field<'a>(crash: &'a str, key: &str) -> Option<&'a str> {
    let rest = crash.split_once(&format!("{key} =>"))?.1.trim_start();
    let end = rest
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .unwrap_or(rest.len());
    Some(&rest[..end])
}

/// One arm's line in the divergence table, against an episode of `calls`.
fn verdict(outcome: &Outcome, calls: usize) -> String {
    match outcome {
        Outcome::Agreed => format!("agreed with all {calls} calls"),
        Outcome::Parted { position, kind } => format!("parted at call {position} ({kind})"),
    }
}
