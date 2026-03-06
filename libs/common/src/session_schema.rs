use crate::error::Error;
use crate::ports::outbound::FileSystem;
use std::path::Path;

pub const SESSION_SCHEMA_VERSION_FILE: &str = "session_schema_version";
pub const SESSION_SCHEMA_LATEST: u32 = 2;

pub fn read_version(fs: &dyn FileSystem, session_dir: &Path) -> Result<u32, Error> {
    let p = session_dir.join(SESSION_SCHEMA_VERSION_FILE);
    let s = fs.read_to_string(&p).map_err(|_| {
        Error::invalid_argument(format!(
            "Session schema version file missing: {} (run scripts/migrate.sh)",
            p.display()
        ))
    })?;
    let v = s.trim().parse::<u32>().map_err(|e| {
        Error::invalid_argument(format!(
            "Invalid session schema version '{}': {} (file: {})",
            s.trim(),
            e,
            p.display()
        ))
    })?;
    Ok(v)
}

pub fn require_latest(fs: &dyn FileSystem, session_dir: &Path) -> Result<(), Error> {
    let v = read_version(fs, session_dir)?;
    if v != SESSION_SCHEMA_LATEST {
        return Err(Error::invalid_argument(format!(
            "Unsupported session schema version {} (latest {}). Run scripts/migrate.sh -s {}",
            v,
            SESSION_SCHEMA_LATEST,
            session_dir.display()
        )));
    }
    Ok(())
}
