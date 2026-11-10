//! GSPro club codes (`DR`, `W3`, `H4`, `I7`, `PW`, `PT`, …): their order in a bag and their display names.
//!
//! The one place the app sorts and names clubs, so another crate can show them the same way.

use crate::i18n::tr;

/// A club's position in a bag, from [`rank`]: compare and sort by it, its encoding is not public.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Rank(u8, u8);

/// Position of a GSPro club code (upper case, as GSPro sends it) in a bag: driver, woods, hybrids,
/// irons, wedges (`AW` as `GW`), putter. A code starting with `W`, `H` or `I` ranks among its letter's
/// clubs by the number after it (0 when none parses as a number); any other code ranks after the
/// putter, no club (`None`) last. Sort with `codes.sort_by_key(|c| club::rank(c.as_deref()))`.
#[must_use]
pub fn rank(code: Option<&str>) -> Rank {
    let Some(code) = code else { return Rank(9, 0) };
    let n = code.get(1..).and_then(|n| n.parse::<u8>().ok()).unwrap_or(0);
    match code {
        "DR" => Rank(0, 0),
        "PW" => Rank(5, 0),
        "GW" | "AW" => Rank(5, 1),
        "SW" => Rank(5, 2),
        "LW" => Rank(5, 3),
        "PT" => Rank(6, 0),
        c if c.starts_with('W') => Rank(1, n),
        c if c.starts_with('H') => Rank(2, n),
        c if c.starts_with('I') => Rank(3, n),
        _ => Rank(8, 0),
    }
}

/// GSPro club code (upper case, as GSPro sends it) → display name in the active language ("7-Iron",
/// "Eisen 7"). A code of two or more characters starting with `W`, `H` or `I` is named by its letter
/// with the rest as the number (`"WX"` → "X-Wood"); any other code (also `"W"` or `"AW"`) as it is.
#[must_use]
pub fn name(code: &str) -> String {
    let num = |s: &str| s[1..].to_string();
    match code {
        "DR" => "Driver".into(),
        "PW" => "Pitching Wedge".into(),
        "GW" => "Gap Wedge".into(),
        "SW" => "Sand Wedge".into(),
        "LW" => "Lob Wedge".into(),
        "PT" => "Putter".into(),
        c if c.len() >= 2 && c.starts_with('W') => tr("{n}-Wood").replace("{n}", &num(c)),
        c if c.len() >= 2 && c.starts_with('H') => tr("{n}-Hybrid").replace("{n}", &num(c)),
        c if c.len() >= 2 && c.starts_with('I') => tr("{n}-Iron").replace("{n}", &num(c)),
        c => c.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{self, Language};

    #[test]
    fn clubs_sort_in_bag_order() {
        let mut codes = vec![Some("PW"), None, Some("I7"), Some("H4"), Some("DR"), Some("LW"), Some("W3"), Some("I10"), Some("GW"), Some("PT")];
        codes.sort_by_key(|c| rank(*c));
        assert_eq!(codes, [Some("DR"), Some("W3"), Some("H4"), Some("I7"), Some("I10"), Some("PW"), Some("GW"), Some("LW"), Some("PT"), None]);
    }

    #[test]
    fn approach_wedge_ranks_as_gap_wedge() {
        assert_eq!(rank(Some("AW")), rank(Some("GW")));
    }

    #[test]
    fn unknown_code_ranks_after_the_putter_before_no_club() {
        assert!(rank(Some("XY")) > rank(Some("PT")));
        assert!(rank(Some("XY")) < rank(None));
    }

    #[test]
    fn names_in_both_languages() {
        i18n::set(&Language::new("en"));
        assert_eq!(name("DR"), "Driver");
        assert_eq!(name("I7"), "7-Iron");
        assert_eq!(name("W3"), "3-Wood");
        assert_eq!(name("H4"), "4-Hybrid");
        i18n::set(&Language::new("de"));
        assert_eq!((name("I7"), name("W3"), name("H4")), ("Eisen 7".into(), "Holz 3".into(), "Hybrid 4".into()));
        assert_eq!(name("DR"), "Driver");
        i18n::set(&Language::new("en"));
    }

    #[test]
    fn unknown_code_is_named_as_it_is() {
        assert_eq!(name("XY"), "XY");
        assert_eq!(name("AW"), "AW", "the bag order knows AW, the names don't (today's app)");
    }
}
