mod backcompat;
mod snapshot;
mod targets;
mod utils;

use std::sync::Mutex;

use anyhow::anyhow;
use clap::{Parser, Subcommand};
use once_cell::sync::Lazy;

use crate::{
    snapshot::{test_snapshot, Snapshot, ALL_SNAPSHOT_TESTS, SNAPSHOT_TESTS_DIRECTORY},
    utils::{run_capturing_stdout, run_command},
};

static ALL_ERRORS: Lazy<Mutex<Vec<String>>> = Lazy::new(|| Mutex::new(vec![]));

#[derive(Debug, Parser)]
struct Options {
    #[command(subcommand)]
    cmd: TestCommand,

    /// Treat compiler warnings as errors (`RUSTFLAGS="--deny warnings"`)
    #[arg(long, short)]
    deny_warnings: bool,

    /// Keep target toolchains that were installed as dependency
    #[arg(long, short)]
    keep_targets: bool,
}

#[derive(Debug, Subcommand)]
#[allow(clippy::enum_variant_names)]
enum TestCommand {
    TestAll,
    TestBackcompat,
    TestBook,
    TestCross,
    TestHost,
    TestLint,
    TestUi,
    /// Run snapshot tests or optionally overwrite the expected output
    TestSnapshot {
        /// Overwrite the expected output instead of comparing it.
        #[arg(long)]
        overwrite: bool,
        /// Runs a single snapshot test in Debug mode
        single: Option<Snapshot>,
    },
}

fn main() -> anyhow::Result<()> {
    let opt = Options::parse();
    let mut added_targets = None;

    match opt.cmd {
        TestCommand::TestBook => test_book(),
        TestCommand::TestBackcompat => backcompat::test(),
        TestCommand::TestHost => test_host(opt.deny_warnings),
        TestCommand::TestLint => test_lint(),
        TestCommand::TestUi => test_ui(),

        // following tests need to install additional targets
        cmd => {
            added_targets = Some(targets::install().expect("Error while installing required targets"));
            match cmd {
                TestCommand::TestCross => test_cross(),
                TestCommand::TestSnapshot { overwrite, single } => {
                    test_snapshot(overwrite, single);
                }
                TestCommand::TestAll => {
                    test_host(opt.deny_warnings);
                    test_cross();
                    test_snapshot(false, None);
                    backcompat::test();
                    test_book();
                    test_lint();
                }
                _ => unreachable!("get handled in outer `match`"),
            }
        }
    }

    if let Some(added_targets) = added_targets {
        if !opt.keep_targets && !added_targets.is_empty() {
            targets::uninstall(added_targets)
        }
    }

    let all_errors = ALL_ERRORS.lock().unwrap();
    if !all_errors.is_empty() {
        eprintln!();
        Err(anyhow!("😔 some tests failed: {:#?}", all_errors))
    } else {
        Ok(())
    }
}

fn do_test(test: impl FnOnce() -> anyhow::Result<()>, context: &str) {
    test().unwrap_or_else(|e| ALL_ERRORS.lock().unwrap().push(format!("{context}: {e}")));
}

fn test_host(deny_warnings: bool) {
    println!("🧪 host");

    let env = if deny_warnings {
        vec![("RUSTFLAGS", "--deny warnings")]
    } else {
        vec![]
    };

    do_test(|| run_command("cargo", &["check"], None, &env), "host");

    do_test(
        || run_command("cargo", &["check", "--features", "unstable-test"], None, &env),
        "host",
    );

    do_test(
        || run_command("cargo", &["check", "--features", "alloc"], None, &env),
        "host",
    );

    do_test(
        || run_command("cargo", &["check", "--features", "ip_in_core"], None, &env),
        "host",
    );

    do_test(
        || run_command("cargo", &["test", "--features", "unstable-test"], None, &[]),
        "host",
    );

    do_test(
        || run_command("cargo", &["test", "--features", "unstable-test,alloc"], None, &[]),
        "host",
    );
}

fn test_cross() {
    println!("🧪 cross");
    let targets = [
        "thumbv6m-none-eabi",
        "thumbv8m.base-none-eabi",
        "riscv32i-unknown-none-elf",
    ];

    for target in &targets {
        do_test(
            || run_command("cargo", &["check", "--target", target, "-p", "defmt"], None, &[]),
            "cross",
        );
        do_test(
            || {
                run_command(
                    "cargo",
                    &["check", "--target", target, "-p", "defmt", "--features", "alloc"],
                    None,
                    &[],
                )
            },
            "cross",
        );
        do_test(
            || {
                run_command(
                    "cargo",
                    &[
                        "check",
                        "--target",
                        target,
                        "-p",
                        "defmt",
                        "--features",
                        "ip_in_core",
                    ],
                    None,
                    &[],
                )
            },
            "cross",
        );
    }

    do_test(
        || {
            run_command(
                "cargo",
                &[
                    "check",
                    "--target",
                    "thumbv6m-none-eabi",
                    "--workspace",
                    "--exclude",
                    "defmt-itm",
                    "--exclude",
                    "firmware",
                ],
                Some("firmware"),
                &[],
            )
        },
        "cross",
    );

    do_test(
        || {
            run_command(
                "cargo",
                &["check", "--target", "thumbv7em-none-eabi"],
                Some("firmware"),
                &[],
            )
        },
        "cross",
    );

    do_test(
        || {
            run_command(
                "cargo",
                &[
                    "check",
                    "--target",
                    "thumbv6m-none-eabi",
                    "--features",
                    "print-defmt",
                ],
                Some("firmware/panic-probe"),
                &[],
            )
        },
        "cross",
    );

    do_test(
        || {
            run_command(
                "cargo",
                &[
                    "check",
                    "--target",
                    "thumbv6m-none-eabi",
                    "--features",
                    "print-rtt",
                ],
                Some("firmware/panic-probe"),
                &[],
            )
        },
        "cross",
    );

    do_test(
        || {
            run_command(
                "cargo",
                &["clippy", "--target", "thumbv7m-none-eabi", "--", "-D", "warnings"],
                Some("firmware/"),
                &[],
            )
        },
        "lint",
    );
}

fn test_book() {
    println!("🧪 book");
    do_test(|| run_command("cargo", &["clean"], None, &[]), "book");

    do_test(
        || {
            run_command(
                "cargo",
                &[
                    "build",
                    "-p",
                    "defmt",
                    "-p",
                    "defmt-decoder",
                    "--features",
                    "unstable-test",
                ],
                None,
                &[],
            )
        },
        "book",
    );

    do_test(
        || run_command("cargo", &["build", "-p", "cortex-m"], Some("firmware"), &[]),
        "book",
    );

    do_test(
        || {
            run_command(
                "mdbook",
                &[
                    "test",
                    "-L",
                    "../target/debug",
                    "-L",
                    "../target/debug/deps",
                    "-L",
                    "../firmware/target/debug",
                    "-L",
                    "../firmware/target/debug/deps",
                ],
                Some("book"),
                // logging macros need this but mdbook, not being Cargo, doesn't set the env var so
                // we use a dummy value
                &[("CARGO_CRATE_NAME", "krate")],
            )
        },
        "book",
    );
}

fn test_lint() {
    println!("🧪 lint");

    // rustfmt
    for cwd in [None, Some("firmware/")] {
        do_test(
            || run_command("cargo", &["fmt", "--", "--check"], cwd, &[]),
            "lint",
        );
    }

    // clippy
    do_test(
        || run_command("cargo", &["clippy", "--", "-D", "warnings"], None, &[]),
        "lint",
    );
}

fn test_ui() {
    println!("🧪 lint");
    do_test(
        || run_command("cargo", &["test"], Some("firmware/defmt-test/macros"), &[]),
        "ui",
    );
}
