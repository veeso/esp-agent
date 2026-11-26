//! Exercises `target!` without a standard prelude or a caller-side `alloc` import.

#![no_std]

#[test]
fn inserts_discovered_target_with_borrowed_aliases() {
    use esp_agent::{DynamicTargetCatalog, target};

    fn insert(catalog: &mut DynamicTargetCatalog, id: u32, name: &str, aliases: &[&str]) {
        catalog
            .insert(target!(id, name, replies = aliases.iter().copied()))
            .unwrap();
    }

    let mut catalog = DynamicTargetCatalog::new();
    insert(&mut catalog, 1, "desk lamp", &["desk one"]);
    assert_eq!(catalog.name(1), Some("desk lamp"));
    assert!(catalog.matches_target(1, "desk one"));
}
