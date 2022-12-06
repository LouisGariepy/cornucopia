use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::{conn, container, error::Error, generate_live, generate_managed, CodegenSettings};

/// Command line interface to interact with Cornucopia SQL.
#[derive(Parser, Debug)]
#[clap(version)]
struct Args {
    /// Use `podman` instead of `docker`
    #[clap(short, long)]
    podman: bool,
    /// Folder containing the queries
    #[clap(short, long, default_value = "queries/")]
    queries_path: PathBuf,
    /// Destination folder for generated modules
    #[clap(short, long, default_value = "src/cornucopia.rs")]
    destination: PathBuf,
    #[clap(subcommand)]
    action: Action,
    /// Generate synchronous rust code. Async otherwise.
    #[clap(long)]
    sync: bool,
    /// Derive serde's `Serialize` trait for generated types.
    #[clap(long)]
    serialize: bool,
}

#[derive(Debug, Subcommand)]
enum Action {
    /// Generate your modules against your own db
    Live {
        /// Postgres url to the database
        url: String,
    },
    /// Generate your modules against schema files
    Schema {
        /// SQL files containing the database schema
        schema_files: Vec<PathBuf>,
    },
}

// Main entrypoint of the CLI. Parses the args and calls the appropriate routines.
pub fn run() -> Result<(), Error> {
    let Args {
        podman,
        queries_path,
        destination,
        action,
        sync,
        serialize,
    } = Args::parse();

    match action {
        Action::Live { url } => {
            let mut client = conn::from_url(&url)?;
            generate_live(
                &mut client,
                &queries_path,
                Some(&destination),
                CodegenSettings {
                    is_async: !sync,
                    derive_ser: serialize,
                },
            )?;
        }
        Action::Schema { schema_files } => {
            // Run the generate command. If the command is unsuccessful, cleanup Cornucopia's container
            if let Err(e) = generate_managed(
                queries_path,
                schema_files.as_slice(),
                Some(destination),
                podman,
                CodegenSettings {
                    is_async: !sync,
                    derive_ser: serialize,
                },
            ) {
                container::cleanup(podman).ok();
                return Err(e);
            }
        }
    };
    Ok(())
}
