//! Club order and names as another crate uses them: only the library's public interface, without `gui`.

use openlaunch_connect::club;
use openlaunch_connect::i18n::{self, Language};

#[test]
fn an_extension_sorts_clubs_in_bag_order() {
    let mut codes: Vec<Option<String>> = ["PW", "", "I7", "H4", "DR", "LW", "W3", "I10", "GW", "PT", "XY", "AW"]
        .into_iter()
        .map(|c| (!c.is_empty()).then(|| c.to_string()))
        .collect();
    codes.sort_by_key(|c| club::rank(c.as_deref()));
    let sorted: Vec<&str> = codes.iter().map(|c| c.as_deref().unwrap_or("-")).collect();
    assert_eq!(sorted, ["DR", "W3", "H4", "I7", "I10", "PW", "GW", "AW", "LW", "PT", "XY", "-"]);
}

/// One test for both languages: the language is process-wide outside the library's unit tests.
#[test]
fn an_extension_names_clubs_in_the_active_language() {
    i18n::set(&Language::new("en"));
    let names = |codes: &[&str]| codes.iter().map(|c| club::name(c)).collect::<Vec<_>>();
    assert_eq!(names(&["DR", "I7", "W3", "H4", "XY"]), ["Driver", "7-Iron", "3-Wood", "4-Hybrid", "XY"]);
    i18n::set(&Language::new("de"));
    assert_eq!(names(&["I7", "W3", "H4", "DR", "XY"]), ["Eisen 7", "Holz 3", "Hybrid 4", "Driver", "XY"]);
    i18n::set(&Language::new("en"));
}
