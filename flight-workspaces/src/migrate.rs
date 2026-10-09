// SPDX-License-Identifier: MIT

use crate::LoadError;

/// One schema upgrade, `steps[n]` taking version `n + 1` to `n + 2`. It edits the parsed table
/// and sets nothing else: the version number is advanced by [`run`].
pub type Step = fn(&mut toml::Table) -> Result<(), String>;

/// The upgrade path. Empty while version 1 is the only schema; a new schema version adds a step
/// here and a test that loads the previous version's file.
pub const STEPS: &[Step] = &[];

/// Bring `table` from `from` to the version `steps` leads to.
pub(crate) fn run(table: &mut toml::Table, from: u32, steps: &[Step]) -> Result<(), LoadError> {
    let mut version = from;
    for step in steps.iter().skip(from.saturating_sub(1) as usize) {
        step(table).map_err(LoadError::Migration)?;
        version = version.saturating_add(1);
        table.insert(
            "schema_version".to_owned(),
            toml::Value::Integer(i64::from(version)),
        );
    }
    Ok(())
}
