use std::path::Path;

use miette::NamedSource;
use postgres::Client;

use crate::utils::db_err;

use self::error::Error;

/// Loads PostgreSQL schemas into a database.
///
/// Takes a list of file paths as parameter and loads them in their given order.
pub fn load_schema<P: AsRef<Path>>(client: &mut Client, paths: &[P]) -> Result<(), Error> {
    for path in paths {
        let sql = std::fs::read_to_string(path).map_err(|err| Error::Io {
            path: path.as_ref().to_string_lossy().to_string(),
            err,
        })?;
        client.batch_execute(&sql).map_err(|err| {
            let msg = format!("{:#}", err);
            let src = NamedSource::new(path.as_ref().to_string_lossy(), sql);
            if let Some((position, msg, help)) = db_err(&err) {
                Error::Postgres {
                    msg,
                    help,
                    src,
                    err_span: Some((position as usize..position as usize).into()),
                }
            } else {
                Error::Postgres {
                    msg,
                    help: None,
                    src,
                    err_span: None,
                }
            }
        })?;
    }
    Ok(())
}

pub(crate) mod error {
    use miette::{Diagnostic, NamedSource, SourceSpan};
    use thiserror::Error as ThisError;

    #[derive(Debug, ThisError, Diagnostic)]
    pub enum Error {
        #[error("Could not read schema `{path}`: ({err})")]
        Io { path: String, err: std::io::Error },
        #[error("Could not execute schema: {msg}")]
        Postgres {
            msg: String,
            #[source_code]
            src: NamedSource,
            #[help]
            help: Option<String>,
            #[label("error occurs near this location")]
            err_span: Option<SourceSpan>,
        },
    }
}
