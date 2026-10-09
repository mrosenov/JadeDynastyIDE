//! Static task templates stored in `tasks.data` and its numbered packs.

pub mod analyze;
pub mod browser;
pub mod compare;
pub mod container;
pub mod dialogs;
pub mod edit;
pub mod json;
pub mod layout;
pub mod problems;
pub mod schema;
pub mod save;
pub mod search;
pub mod structures;
pub mod translate;
pub mod v165;
pub mod v172;
pub mod v184;

use schema::Schema;

pub const SUPPORTED_VERSIONS: [u32; 3] = [v165::VERSION, v172::VERSION, v184::VERSION];

pub fn supported_versions() -> &'static [u32] {
    &SUPPORTED_VERSIONS
}

pub fn closest_schema_version(version: u32) -> u32 {
    *SUPPORTED_VERSIONS.iter().min_by_key(|candidate| (candidate.abs_diff(version), **candidate)).unwrap()
}

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
