use glib::prelude::*;

// Exercise the FFI out-pointer with optimizations enabled. Before gtk-rs-core
// PR #1343, g_variant_get_child wrote through an immutable Rust reference.
#[test]
fn optimized_string_iteration_preserves_all_borrowed_values() {
    let values = ["first", "", "Español", "日本語", "last"];
    let variant = values.to_variant();
    assert_eq!(
        variant.array_iter_str().unwrap().collect::<Vec<_>>(),
        values
    );
    assert_eq!(
        variant.array_iter_str().unwrap().rev().collect::<Vec<_>>(),
        values.iter().rev().copied().collect::<Vec<_>>()
    );
    let mut mixed = variant.array_iter_str().unwrap();
    assert_eq!(mixed.next(), Some("first"));
    assert_eq!(mixed.next_back(), Some("last"));
    assert_eq!(mixed.nth(1), Some("Español"));
    assert_eq!(mixed.next_back(), Some("日本語"));
    assert_eq!(mixed.next(), None);
    assert_eq!(mixed.next_back(), None);
}

#[test]
fn optimized_empty_and_singleton_iteration_are_fused() {
    let empty = Vec::<String>::new().to_variant();
    let mut empty_iter = empty.array_iter_str().unwrap();
    assert_eq!(empty_iter.next(), None);
    assert_eq!(empty_iter.next_back(), None);
    let singleton = ["only"].to_variant();
    let mut iter = singleton.array_iter_str().unwrap();
    assert_eq!(iter.next_back(), Some("only"));
    assert_eq!(iter.next(), None);
    assert_eq!(iter.next_back(), None);
}
