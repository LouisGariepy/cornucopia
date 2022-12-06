mod cli;
mod codegen;
mod error;
mod load_schema;
mod parser;
mod prepare_queries;
mod read_queries;
mod type_registrar;
mod utils;
mod validation;

/// Helpers to establish connections to database instances.
pub mod conn;
/// High-level interfaces to work with Cornucopia's container manager.
pub mod container;

use std::path::Path;

use postgres::Client;

use codegen::generate as generate_internal;
use error::WriteOutputError;
use parser::parse_query_module;
use prepare_queries::prepare;
use read_queries::read_query_modules;

#[doc(hidden)]
pub use cli::run;

pub use error::Error;
pub use load_schema::load_schema;

/// Struct containing the settings for code generation.
#[derive(Clone, Copy)]
pub struct CodegenSettings {
    pub is_async: bool,
    pub derive_ser: bool,
}

/// Generates Rust queries from PostgreSQL queries located at `queries_path`,
/// using a live database managed by you. If some `destination` is given,
/// the generated code will be written at that path. Code generation settings are
/// set using the `settings` parameter.
pub fn generate_live<P: AsRef<Path>>(
    client: &mut Client,
    queries_path: P,
    destination: Option<P>,
    settings: CodegenSettings,
) -> Result<String, Error> {
    // Read
    let modules = read_query_modules(queries_path.as_ref())?
        .into_iter()
        .map(parse_query_module)
        .collect::<Result<_, parser::error::Error>>()?;
    // Generate
    let prepared_modules = prepare(client, modules)?;
    let generated_code = generate_internal(prepared_modules, settings);
    // Write
    if let Some(d) = destination {
        write_generated_code(d.as_ref(), &generated_code)?;
    };

    Ok(generated_code)
}

/// Generates Rust queries from PostgreSQL queries located at `queries_path`, using
/// a container managed by cornucopia. The database schema is created using `schema_files`.
/// If some `destination` is given, the generated code will be written at that path.
/// Code generation settings are set using the `settings` parameter.
///
/// By default, the container manager is Docker, but Podman can be used by setting the
/// `podman` parameter to `true`.
pub fn generate_managed<P: AsRef<Path>>(
    queries_path: P,
    schema_files: &[P],
    destination: Option<P>,
    podman: bool,
    settings: CodegenSettings,
) -> Result<String, Error> {
    // Read
    let modules = read_query_modules(queries_path.as_ref())?
        .into_iter()
        .map(parse_query_module)
        .collect::<Result<_, parser::error::Error>>()?;
    container::setup(podman)?;
    let mut client = conn::cornucopia_conn()?;
    load_schema(&mut client, schema_files)?;
    let prepared_modules = prepare(&mut client, modules)?;
    let generated_code = generate_internal(prepared_modules, settings);
    container::cleanup(podman)?;

    if let Some(destination) = destination {
        write_generated_code(destination.as_ref(), &generated_code)?;
    };

    Ok(generated_code)
}

fn write_generated_code(destination: &Path, generated_code: &str) -> Result<(), Error> {
    Ok(
        std::fs::write(destination, generated_code).map_err(|err| WriteOutputError {
            err,
            file_path: destination.to_owned(),
        })?,
    )
}
