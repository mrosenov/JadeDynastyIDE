//! Static task templates stored in `tasks.data` and its numbered packs.

pub mod browser;
pub mod container;
pub mod edit;
pub mod schema;
pub mod save;
pub mod structures;
pub mod v165;
pub mod v172;
pub mod v184;

use schema::Schema;

/// Returns a schema only after that task version has passed a complete,
/// byte-exact real-file round trip.
pub fn schema_for_version(version: u32) -> Result<Schema, String> {
    match version {
        v165::VERSION => Ok(v165::schema()),
        v172::VERSION => Ok(v172::schema()),
        v184::VERSION => Ok(v184::schema()),
        _ => Err(format!(
            "tasks.data v{version} is not supported yet; supported versions are v165, v172 and v184"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_only_verified_task_versions() {
        for version in [165, 172, 184] {
            assert!(schema_for_version(version).is_ok());
        }
        assert!(schema_for_version(203).unwrap_err().contains("not supported"));
    }
}
