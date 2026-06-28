#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct DatabaseSchemaVersion(pub u32);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatabaseMigration {
    pub target_version: DatabaseSchemaVersion,
    pub statements: &'static [&'static str],
}

impl DatabaseMigration {
    pub const fn new(
        target_version: DatabaseSchemaVersion,
        statements: &'static [&'static str],
    ) -> Self {
        Self {
            target_version,
            statements,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_versions_sort_in_migration_order() {
        assert!(DatabaseSchemaVersion(1) < DatabaseSchemaVersion(2));
    }
}
