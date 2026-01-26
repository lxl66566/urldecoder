use std::{io, path::PathBuf};

use snafu::Snafu;

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum Error {
    #[snafu(display("Failed to open input file {}: {}", path.display(), source))]
    OpenInput { path: PathBuf, source: io::Error },

    #[snafu(display("Failed to read input data: {}", source))]
    ReadInput { source: io::Error },

    #[snafu(display("Failed to decode: {}", source))]
    Decode { source: io::Error },

    #[snafu(display("Failed to write output data: {}", source))]
    WriteOutput { source: io::Error },

    #[snafu(display("Failed to create temporary file in {}: {}", dir.display(), source))]
    CreateTemp { dir: PathBuf, source: io::Error },

    #[snafu(display("Failed to persist temporary file to {}: {}", path.display(), source))]
    PersistTemp {
        path: PathBuf,
        source: tempfile::PersistError,
    },

    #[snafu(display("Failed to write back to original file {}: {}", path.display(), source))]
    WriteBack { path: PathBuf, source: io::Error },

    #[snafu(display("Invalid UTF-8 sequence: {}", source))]
    InvalidUtf8 { source: simdutf8::basic::Utf8Error },
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
