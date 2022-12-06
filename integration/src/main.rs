use std::{
    borrow::Cow,
    fmt::Display,
    io::Write,
    process::{Command, ExitCode, Stdio},
};

use clap::Parser;
use cornucopia::{container, CodegenSettings, Error};
use owo_colors::OwoColorize;

/// Start cornucopia test runner
#[derive(Parser, Debug)]
#[clap(version)]
struct Args {
    /// Format test descriptors and update error msg
    #[clap(long)]
    apply_errors: bool,
    /// Update the project's generated code
    #[clap(long)]
    apply_codegen: bool,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct ErrorTestSuite<'a> {
    #[serde(borrow)]
    test: Vec<ErrorTest<'a>>,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct ErrorTest<'a> {
    name: &'a str,
    query: Option<&'a str>,
    schema: Option<&'a str>,
    query_name: Option<&'a str>,
    error: Cow<'a, str>,
}

#[derive(serde::Deserialize)]
struct CodegenTestSuite<'a> {
    #[serde(borrow)]
    codegen: Vec<CodegenTest<'a>>,
}

#[derive(serde::Deserialize)]
struct CodegenTest<'a> {
    name: &'a str,
    base_path: &'a str,
    queries: Option<&'a str>,
    destination: Option<&'a str>,
    sync: Option<bool>,
    derive_ser: Option<bool>,
    run: Option<Run>,
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum Run {
    Bool(bool),
    Path(String),
}

fn main() -> ExitCode {
    let args = Args::parse();
    if test(args) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Print error to stderr
fn display<T, E: Display>(result: Result<T, E>) -> Result<T, E> {
    if let Err(err) = &result {
        eprintln!("{}", err);
    }
    result
}

// Run test, return true if all test are successful
fn test(
    Args {
        apply_errors,
        apply_codegen,
    }: Args,
) -> bool {
    // Start by removing previous container if it was left open
    container::cleanup(false).ok();
    container::setup(false).unwrap();
    let successful = std::panic::catch_unwind(|| {
        let mut client = cornucopia::conn::cornucopia_conn().unwrap();
        display(run_errors_test(&mut client, apply_errors)).unwrap()
            && display(run_codegen_test(&mut client, apply_codegen)).unwrap()
    });
    container::cleanup(false).unwrap();
    successful.unwrap()
}

/// Reset the current database
fn reset_db(client: &mut postgres::Client) -> Result<(), postgres::Error> {
    client.batch_execute("DROP SCHEMA public CASCADE;CREATE SCHEMA public;")
}

// Common schema to all error tests
const SCHEMA_BASE: &str = "CREATE TABLE author (id SERIAL, name TEXT);\n";

/// Run errors test, return true if all test are successful
fn run_errors_test(
    client: &mut postgres::Client,
    apply: bool,
) -> Result<bool, Box<dyn std::error::Error>> {
    let mut successful = true;

    let got_msg = if apply {
        "Apply:".bright_black()
    } else {
        "Got:".bright_black()
    };
    let expected_msg = if apply {
        "Previous:".bright_black()
    } else {
        "Expected:".bright_black()
    };

    let original_pwd = std::env::current_dir().unwrap();
    for file in std::fs::read_dir("fixtures/errors")? {
        let file = file?;
        let name = file.file_name().to_string_lossy().to_string();
        let content = std::fs::read_to_string(file.path())?;
        let mut suite: ErrorTestSuite = toml::from_str(&content)?;

        println!("{} {}", "[error]".magenta(), name.magenta());
        for test in &mut suite.test {
            // Generate file tree path
            let temp_dir = tempfile::tempdir()?;

            // Reset db
            reset_db(client)?;

            // We need to change current dir for error path to always be the same
            std::env::set_current_dir(&temp_dir)?;

            // Generate schema
            std::fs::write(
                "schema.sql",
                [SCHEMA_BASE, test.schema.unwrap_or_default()].concat(),
            )?;

            // Generate queries files
            std::fs::create_dir("queries")?;
            let name = test.query_name.unwrap_or("test.sql");
            std::fs::write(&format!("queries/{name}"), test.query.unwrap_or_default())?;

            // Run codegen
            let result: Result<(), cornucopia::Error> = (|| {
                cornucopia::load_schema(client, &["schema.sql"])?;
                cornucopia::generate_live(
                    client,
                    "queries",
                    None,
                    CodegenSettings {
                        is_async: false,
                        derive_ser: false,
                    },
                )?;
                Ok(())
            })();

            let err = result.err().map(Error::report).unwrap_or_default();
            if err.trim() == test.error.trim() {
                println!("{} {}", test.name, "OK".green());
            } else {
                successful = false;
                println!(
                    "{} {}\n{}\n{}\n{}\n{}",
                    test.name,
                    "ERR".red(),
                    expected_msg,
                    test.error,
                    got_msg,
                    err,
                );
            }
            if apply {
                test.error = Cow::Owned(err.trim().to_string());
            }
            std::env::set_current_dir(&original_pwd)?;
        }

        if apply {
            // Format test descriptor and update error message if needed
            let edited = toml::to_string_pretty(&suite)?;
            std::fs::write(file.path(), edited)?;
        }
    }
    Ok(successful)
}

// Run codegen test, return true if all test are successful
fn run_codegen_test(
    client: &mut postgres::Client,
    apply: bool,
) -> Result<bool, Box<dyn std::error::Error>> {
    let mut successful = true;
    let original_pwd = std::env::current_dir()?;

    for file in std::fs::read_dir("fixtures/codegen")? {
        let file = file?;
        let name = file.file_name().to_string_lossy().to_string();
        let content = std::fs::read_to_string(file.path())?;
        println!("{} {}", "[codegen]".magenta(), name.magenta());

        let suite: CodegenTestSuite = toml::from_str(&content)?;

        for codegen_test in suite.codegen {
            std::env::set_current_dir(format!("../{}", codegen_test.base_path))?;
            let queries_path = codegen_test.queries.unwrap_or("queries");
            let schema_path = "schema.sql";
            let destination = codegen_test.destination.unwrap_or("src/cornucopia.rs");
            let is_async = !codegen_test.sync.unwrap_or(false);
            let derive_ser = codegen_test.derive_ser.unwrap_or(false);

            // Load schema
            reset_db(client)?;
            cornucopia::load_schema(client, &[schema_path])?;

            // If `--apply`, then the code will be regenerated.
            // Otherwise, it is only checked.
            if apply {
                // Generate
                cornucopia::generate_live(
                    client,
                    queries_path,
                    Some(destination),
                    CodegenSettings {
                        is_async,
                        derive_ser,
                    },
                )
                .map_err(Error::report)?;
                // Format the generated file
                Command::new("rustfmt")
                    .args(["--edition", "2021"])
                    .arg(destination)
                    .output()?;
            } else {
                // Get currently checked-in generate file
                let old_codegen = std::fs::read_to_string(destination).unwrap_or_default();
                // Generate new file
                let new_codegen = cornucopia::generate_live(
                    client,
                    queries_path,
                    None,
                    CodegenSettings {
                        is_async,
                        derive_ser,
                    },
                )
                .map_err(Error::report)?;
                // Format the generated code string by piping to rustfmt
                let mut rustfmt = Command::new("rustfmt")
                    .args(["--edition", "2021"])
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .spawn()?;
                rustfmt
                    .stdin
                    .as_mut()
                    .unwrap()
                    .write_all(new_codegen.as_bytes())?;
                let formated_new_codegen =
                    String::from_utf8(rustfmt.wait_with_output()?.stdout).unwrap();

                // If the newly generated file differs from
                // the currently checked in one, return an error.
                if old_codegen != formated_new_codegen {
                    Err("\"{destination}\" is outdated")?;
                }
            }
            println!("(generate) {} {}", codegen_test.name, "OK".green());

            // Run code
            let run = match codegen_test.run.unwrap_or(Run::Bool(false)) {
                Run::Bool(bool) => bool,
                Run::Path(path) => {
                    // Switch directory
                    std::env::set_current_dir(&original_pwd)?;
                    std::env::set_current_dir(&format!("../{}", path))?;
                    true
                }
            };
            if run {
                let result = Command::new("cargo").arg("run").output()?;
                if result.status.success() {
                    println!("(run) {} {}", codegen_test.name, "OK".green());
                } else {
                    successful = false;
                    println!(
                        " {}\n{}",
                        "ERR".red(),
                        String::from_utf8_lossy(&result.stderr)
                            .as_ref()
                            .bright_black()
                    );
                }
            }

            // Move back to original directory
            std::env::set_current_dir(&original_pwd)?;
        }
    }

    Ok(successful)
}

#[cfg(test)]
mod test {
    use crate::test;

    #[test]
    fn run() {
        assert!(test(crate::Args {
            apply_errors: false,
            apply_codegen: false
        }))
    }
}
