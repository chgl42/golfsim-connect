//! UI translations. The English text is the key (gettext style): `tr("Settings")`
//! returns the German text when German is active and falls back to English for
//! anything not in the table. The log stays English on purpose (support/debugging).

use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

/// Language setting as stored in the settings file.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    /// Follow the macOS language.
    #[default]
    System,
    English,
    German,
}

impl Language {
    pub const ALL: [Self; 3] = [Self::System, Self::English, Self::German];

    /// Whether this setting means German.
    #[must_use]
    pub fn is_german(self) -> bool {
        match self {
            Self::English => false,
            Self::German => true,
            Self::System => sys_locale::get_locale().is_some_and(|l| l.to_lowercase().starts_with("de")),
        }
    }

    /// Name shown in the language picker (each language in its own language).
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::System => tr("System"),
            Self::English => "English",
            Self::German => "Deutsch",
        }
    }
}

static GERMAN: AtomicBool = AtomicBool::new(false);

/// Switch the UI language.
pub fn set(language: Language) {
    GERMAN.store(language.is_german(), Ordering::Relaxed);
}

#[must_use]
pub fn german() -> bool {
    GERMAN.load(Ordering::Relaxed)
}

/// Translate a fixed UI text.
#[must_use]
pub fn tr(en: &'static str) -> &'static str {
    if german() { de(en).unwrap_or(en) } else { en }
}

/// Whether `text` is `en` in any of the languages (e.g. a default name the user typed in German).
#[must_use]
pub fn is_text(en: &str, text: &str) -> bool {
    text == en || de(en) == Some(text)
}

/// Translate a status text that may carry a trailing detail in parentheses,
/// e.g. "Connected (127.0.0.1:921)" → "Verbunden (127.0.0.1:921)".
#[must_use]
pub fn tr_detail(text: &str) -> String {
    if !german() {
        return text.to_string();
    }
    if let Some(t) = de(text) {
        return t.to_string();
    }
    if let Some(i) = text.rfind(" (")
        && text.ends_with(')')
        && let Some(t) = de(&text[..i])
    {
        return format!("{t}{}", &text[i..]);
    }
    text.to_string()
}

#[allow(clippy::too_many_lines)]
fn de(en: &str) -> Option<&'static str> {
    Some(match en {
        // Navigation and sections
        "PLAY" => "SPIEL",
        "SHOTS" => "SCHLÄGE",
        "LOG" => "LOG",
        "SETTINGS" => "EINSTELLUNGEN",
        "BALL" => "BALL",
        "CLUB" => "SCHLÄGER",
        "STATUS" => "STATUS",

        // Play page
        "Ball Speed" => "Ballgeschw.",
        "Total Spin" => "Spin gesamt",
        "Spin Axis" => "Spinachse",
        "Side Spin" => "Seitenspin",
        "Club Speed" => "Schlägergeschw.",
        "Club AoA" => "Anstellwinkel",
        "Club Path" => "Schwungbahn",
        "Face to Target" => "Schlagfläche",
        "Face to Path" => "Fläche zur Bahn",
        "Smash Factor" => "Smash-Faktor",
        "LEFT" => "LINKS",
        "RIGHT" => "RECHTS",
        "Hand" => "Hand",
        "TEST SHOT IN 3 S" => "TESTSCHLAG IN 3 S",
        "START GSPRO" => "GSPRO STARTEN",
        "Connect to GSPro first" => "Erst mit GSPro verbinden",
        "GSPro is already connected" => "GSPro ist bereits verbunden",
        "No GSPro launcher selected (Settings)" => "Kein GSPro-Starter gewählt (Einstellungen)",
        "No shot yet" => "Noch kein Schlag",
        "{n} s ago" => "vor {n} s",
        "{n} min ago" => "vor {n} min",
        "delivered to GSPro" => "an GSPro übergeben",
        "NOT delivered to GSPro" => "NICHT an GSPro übergeben",
        "Tempo" => "Tempo",
        "Exclude" => "Ausblenden",
        "Count" => "Mitzählen",

        "LAST SHOT" => "LETZTER SCHLAG",
        "{n} sent" => "{n} gesendet",
        "Backswing : downswing (↑ backswing, ↓ downswing)" => "Rückschwung : Abschwung (↑ Rückschwung, ↓ Abschwung)",

        // Status words
        "READY" => "BEREIT",
        "NO R10" => "KEIN R10",
        "WAITING FOR GSPRO" => "WARTE AUF GSPRO",
        "R10 NOT READY" => "R10 NICHT BEREIT",

        "COPY" => "KOPIEREN",
        "The log is empty" => "Das Log ist leer",

        // Shots page
        "This session ({n})" => "Diese Session ({n})",
        "All time" => "Gesamt",
        "EXPORT CSV" => "CSV EXPORT",
        "No shots to export" => "Keine Schläge zum Exportieren",
        "No shots yet." => "Noch keine Schläge.",
        "Club" => "Schläger",
        "Shots" => "Schläge",
        "Ball" => "Ball",
        "Smash" => "Smash",
        "Spin" => "Spin",
        "Axis" => "Achse",
        "Time" => "Zeit",
        "Unknown" => "Unbekannt",
        "Not delivered to GSPro" => "Nicht an GSPro übergeben",
        "Export shots as CSV" => "Schläge als CSV exportieren",
        "Exported {n} shots to {path}" => "{n} Schläge exportiert nach {path}",
        "Export failed: {e}" => "Export fehlgeschlagen: {e}",

        // Settings page
        "– not selected –" => "– nicht gewählt –",
        "Find" => "Suchen",
        "Choose …" => "Auswählen …",
        "Choose GSPro launcher (CrossOver app)" => "GSPro-Starter auswählen (CrossOver-App)",
        "No GSPro launcher found under ~/Applications/CrossOver." => {
            "Kein GSPro-Starter unter ~/Applications/CrossOver gefunden."
        }
        "GSPRO" => "GSPRO",
        "DISPLAY" => "ANZEIGE",
        "Launcher" => "Starter",
        "Address" => "Adresse",
        "Applies after restart" => "Gilt nach Neustart",
        "Notify about problems" => "Bei Problemen benachrichtigen",
        "Autostart" => "Autostart",
        "Open GSPro on start" => "GSPro beim Start öffnen",
        "On top of GSPro, also in full screen." => "Liegt über GSPro, auch im Vollbild.",
        "OFF" => "AUS",
        "Ball distance" => "Ballabstand",
        "Distance from the R10 to the ball; default 2.10 m" => "Abstand vom R10 zum Ball; Standard 2.10 m",
        "Device" => "Gerät",
        "Battery" => "Akku",
        "Not connected" => "Nicht verbunden",
        "Log GSPro messages" => "GSPro-Nachrichten protokollieren",
        "Unsaved changes" => "Ungespeicherte Änderungen",
        "NOTIFICATIONS & UPDATES" => "MITTEILUNGEN & UPDATES",
        "Updates" => "Updates",
        "Check on start" => "Beim Start prüfen",
        "Asks GitHub for the latest release when the app starts" => "Fragt beim Start bei GitHub nach der neuesten Version",
        "Installed: {v}" => "Installiert: {v}",
        "Update" => "Update",
        "Asks GitHub for the latest release now" => "Fragt jetzt bei GitHub nach der neuesten Version",
        "checking …" => "prüfe …",
        "up to date" => "aktuell",
        "update {v} available" => "Update {v} verfügbar",
        "check failed" => "Prüfung fehlgeschlagen",
        "UPDATE {v}" => "UPDATE {v}",
        "A newer version is available – click to open the release page" => {
            "Eine neuere Version ist verfügbar – Klick öffnet die Release-Seite"
        }
        "Language" => "Sprache",
        "System" => "System",
        "Interface" => "Oberfläche",
        "Double height" => "Doppelte Höhe",
        "Reset" => "Zurücksetzen",
        "Status" => "Status",
        "Remove column" => "Spalte entfernen",
        "Empty" => "Leer",
        "Click to choose what this tile shows, drag to move it" => "Klicken wählt, was die Kachel zeigt; Ziehen verschiebt sie",
        "Back to the tiles as they came" => "Zurück zur Standard-Anordnung",
        "Add an empty column at the end" => "Leere Spalte hinten anfügen",
        "+ at the end of the tiles adds a column. The status always comes first." => "+ am Ende der Kacheln fügt eine Spalte hinzu. Der Status steht immer vorn.",
        "Click a tile in the preview to choose what it shows, drag it to move it." => "Kachel in der Vorschau anklicken wählt den Inhalt, Ziehen verschiebt sie – auf einen Trennstrich gezogen in eine neue Spalte.",
        "Distance to the pin" => "Zur Fahne",
        "Carry, total, apex, side and landing angle as GSPro computed them" => "Carry, Gesamt, Scheitelhöhe, Seitlich und Landewinkel so, wie GSPro sie berechnet hat",
        "GSPro results" => "GSPro-Werte",
        "Carry, total, apex and side as GSPro computed them, where it did; else the estimate (*)" => "Carry, Gesamt, Scheitelhöhe und Seitlich so, wie GSPro sie berechnet hat, wo vorhanden; sonst die Schätzung (*)",
        "from GSPro" => "von GSPro",
        "Play the flight again" => "Flug noch einmal abspielen",
        "Width" => "Breite",
        "Mean" => "Mittel",
        "Shape" => "Schlagform",
        "Straight" => "Gerade",
        "Impact from the side" => "Treffmoment Seite",
        "Impact from above" => "Treffmoment Aufsicht",
        "Path" => "Bahn",
        "Range" => "Range",
        "Ball flight" => "Ballflug",
        "Data" => "Daten",
        "3D flight" => "3D-Flug",
        "Tee" => "Abschlag",
        "Side view" => "Seite",
        "Above" => "Oben",
        "No shots saved yet – shots hit with the R10 are saved, test shots aren't." => "Noch keine Schläge gespeichert – gespeichert werden Schläge mit der R10, Testschüsse nicht.",
        "Export {n} shots as CSV" => "{n} Schläge als CSV exportieren",
        "The shots shown, with the filter. Choose the layout:" => "Die gezeigten Schläge, mit Filter. Format wählen:",
        "All values of this app, one row per shot" => "Alle Werte dieser App, eine Zeile pro Schlag",
        "Columns and units like the Garmin Golf app's export" => "Spalten und Einheiten wie der Export der Garmin-Golf-App",
        "Columns like Awesome Golf's export" => "Spalten wie der Export von Awesome Golf",
        "Move the page left" => "Seite nach links",
        "Move the page right" => "Seite nach rechts",
        "Name of the page" => "Name der Seite",
        "Remove the page “{name}”?" => "Seite „{name}“ entfernen?",
        "Its tiles go with it. Your shots are kept." => "Ihre Kacheln verschwinden mit. Deine Schläge bleiben erhalten.",
        "REMOVE" => "ENTFERNEN",
        "None" => "Keine",
        "All" => "Alle",
        "Clubs" => "Schläger",
        "Dispersion" => "Streuung",
        "Correlation" => "Korrelation",
        "Correlation of the two values (−1 to 1) over the shots shown, excluded ones left out" => "Korrelation der beiden Werte (−1 bis 1) über die gezeigten Schläge, ohne ausgeblendete",
        "No shots with these values" => "Keine Schläge mit diesen Werten",
        "CANCEL" => "ABBRECHEN",
        "Reset the shots view?" => "Schläge-Ansicht zurücksetzen?",
        "All pages and tiles go back to how they came. Your shots are kept." => "Alle Seiten und Kacheln kommen in den Auslieferungszustand. Deine Schläge bleiben erhalten.",
        "RESET" => "ZURÜCKSETZEN",
        "RESET SHOTS VIEW" => "SCHLÄGE-ANSICHT ZURÜCKSETZEN",
        "The shots view is as it came" => "Die Schläge-Ansicht ist im Auslieferungszustand",
        "Shots view reset." => "Schläge-Ansicht zurückgesetzt.",
        "Table" => "Tabelle",
        "Details" => "Details",
        "Page" => "Seite",
        "Page {n}" => "Seite {n}",
        "Add a page" => "Seite hinzufügen",
        "DONE" => "FERTIG",
        "CONFIGURE" => "KONFIGURIEREN",
        "Add tile" => "Kachel hinzufügen",
        "Right-click for the page's settings" => "Rechtsklick für die Einstellungen der Seite",
        "Remove page" => "Seite entfernen",
        "Filter" => "Filter",
        "Hide excluded" => "Ausgeblendete verbergen",
        "Clear" => "Aufheben",
        "Count these shots in the stats and exports again" => "Diese Schläge wieder in Statistik und Export mitzählen",
        "Leave these shots out of the stats and exports (e.g. misreads); they stay listed" => "Diese Schläge nicht in Statistik und Export mitzählen (z. B. Fehlmessungen); sie bleiben in der Liste",
        "Show all" => "Alle zeigen",
        "Show only these" => "Nur diese zeigen",
        "{n} selected" => "{n} ausgewählt",
        "No tiles on this page – CONFIGURE adds some." => "Keine Kacheln auf dieser Seite – über KONFIGURIEREN hinzufügen.",
        "Remove tile" => "Kachel entfernen",
        "Move later" => "Nach hinten",
        "Move earlier" => "Nach vorn",
        "Averages" => "Durchschnitt",
        "Side" => "Seitlich",
        "Apex" => "Scheitelhöhe",
        "Descent" => "Landewinkel",
        "excluded" => "ausgeblendet",
        "Carry" => "Carry",
        "Total" => "Gesamt",
        "Estimated from the launch data (no wind, sea level); GSPro may show other numbers" => "Geschätzt aus den Abflugdaten (ohne Wind, auf Meereshöhe); GSPro kann andere Werte zeigen",
        "Too large for this screen" => "Zu groß für diesen Bildschirm",
        "Auto" => "Auto",
        "Units" => "Einheiten",
        "Metric (km/h)" => "Metrisch (km/h)",
        "Imperial (mph)" => "Imperial (mph)",
        "Overlay" => "Overlay",
        "OVERLAY" => "OVERLAY",
        // Misreads (misread::check)
        "Misread? ({r})" => "Fehlmessung? ({r})",
        "Left out on its own: possible misread ({r}). Count takes it back." => "Automatisch ausgeschlossen: mögliche Fehlmessung ({r}). „Mitzählen“ nimmt ihn zurück.",
        "spin" => "Spin",
        "launch angle" => "Abflugwinkel",
        "start direction" => "Startrichtung",
        "spin axis" => "Spinachse",
        "smash factor" => "Smash-Faktor",
        "LAYOUT" => "LAYOUT",
        "CONTENTS" => "INHALTE",
        "TILE {n}" => "KACHEL {n}",
        "Standard" => "Standard",
        "Add a tile overlay" => "Kachel-Overlay hinzufügen",
        "Arrangement of the overlays" => "Anordnung der Overlays",
        "Arrangement: click to switch, rename or save as a new one" => "Anordnung: klicken zum Wechseln, Umbenennen oder als neue speichern",
        "Name" => "Name",
        "Name of the arrangement" => "Name der Anordnung",
        "Save as a new arrangement" => "Als neue Anordnung speichern",
        "A copy of the overlays as they are, to change from there" => "Eine Kopie der Overlays, wie sie sind, zum Weiterändern",
        "Delete arrangement" => "Anordnung löschen",
        "Arrangement {n}" => "Anordnung {n}",
        "Delete the arrangement “{name}”?" => "Anordnung „{name}“ löschen?",
        "Its overlays go with it." => "Ihre Overlays gehen mit.",
        "DELETE" => "LÖSCHEN",
        "Takes this tile off the screen and out of the arrangement" => "Nimmt die Kachel vom Bildschirm und aus der Anordnung",
        "Placed by hand" => "Von Hand platziert",
        "Done – drag it in GSPro" => "Fertig – in GSPro ziehen",
        "Move on screen" => "Auf dem Bildschirm verschieben",
        "Ends moving; the overlay stays where it is" => "Beendet das Verschieben; das Overlay bleibt, wo es ist",
        "Makes the overlay grabbable: drag it to its place in GSPro and let go" => "Macht das Overlay greifbar: in GSPro an seinen Platz ziehen und loslassen",
        "Drag to its place" => "An seinen Platz ziehen",
        "Under the overlays at its place." => "Am selben Platz unter den anderen.",
        "Small" => "Klein",
        "Medium" => "Mittel",
        "Large" => "Groß",
        "Tile" => "Kachel",
        "Shows" => "Zeigt",
        "Camera" => "Kamera",
        "Axes" => "Achsen",
        "This session" => "Diese Session",
        "Club in GSPro" => "Schläger in GSPro",
        "Position" => "Position",
        "Size" => "Größe",
        "Lines" => "Zeilen",
        "Two lines" => "Zwei Zeilen",
        "One line" => "Eine Zeile",
        "Background" => "Hintergrund",
        "Solid" => "Deckend",
        "The status is always shown" => "Der Status wird immer angezeigt",
        "PREVIEW" => "VORSCHAU",
        "Overlay off" => "Overlay aus",
        "No overlay" => "Kein Overlay",
        "off" => "aus",
        "Top left" => "Oben links",
        "Top" => "Oben",
        "Top right" => "Oben rechts",
        "Left" => "Links",
        "Right" => "Rechts",
        "Bottom left" => "Unten links",
        "Bottom" => "Unten",
        "Bottom right" => "Unten rechts",
        "Notifications" => "Mitteilungen",
        "Low R10 battery, R10 errors (e.g. tilted), lost connections, shots not delivered" => {
            "R10-Akku schwach, R10-Fehler (z. B. schief), Verbindungsabbrüche, nicht übergebene Schläge"
        }
        "Test" => "Testen",
        "Sends a notification now. If none appears, allow notifications for OpenLaunch Connect in System Settings → Notifications." => {
            "Sendet jetzt eine Mitteilung. Erscheint keine, Mitteilungen für OpenLaunch Connect in Systemeinstellungen → Mitteilungen erlauben."
        }
        "Notifications are working." => "Mitteilungen funktionieren.",
        "SAVE" => "SPEICHERN",
        "No changes" => "Keine Änderungen",
        "Saved." => "Gespeichert.",
        "Saved. Address and distance apply after the next start." => {
            "Gespeichert. Adresse und Abstand gelten ab dem nächsten Start."
        }
        "Saving failed: {e}" => "Speichern fehlgeschlagen: {e}",

        // Overlay
        "GSPro not connected" => "GSPro nicht verbunden",
        "Last shot NOT delivered to GSPro" => "Letzter Schlag NICHT an GSPro übergeben",
        "Hit away" => "Bereit zum Schlag",

        // Status details from the bridge (state::set_r10 / set_gspro)
        "Searching for R10 …" => "Suche R10 …",
        "Waiting for GSPro …" => "Warte auf GSPro …",
        "Ready – hit away" => "Bereit – Schlag frei",
        "Ready – hit away (demo)" => "Bereit – Schlag frei (Demo)",
        "Not found – R10 on? iPhone Bluetooth off?" => "Nicht gefunden – R10 an? iPhone-Bluetooth aus?",
        "Connected, handshake in progress …" => "Verbunden, Handshake läuft …",
        "Connection lost – reconnecting …" => "Verbindung verloren – verbinde neu …",
        "No handshake – confirm pairing dialog?" => "Kein Handshake – Kopplungsdialog bestätigen?",
        "Connected, arming …" => "Verbunden, wird scharf geschaltet …",
        "Standby – waking …" => "Standby – wird geweckt …",
        "Device error" => "Gerätefehler",
        "Measuring shot …" => "Messe Schlag …",
        "Interference test …" => "Störungstest …",
        "R10 is tilted – please level it" => "R10 steht schief – bitte gerade ausrichten",
        "R10 overheating" => "R10 überhitzt",
        "Radar saturated (too close to wall/net?)" => "Radar übersteuert (zu nah an Wand/Netz?)",
        "Unknown device error" => "Unbekannter Gerätefehler",
        "Connected" => "Verbunden",
        "Waiting for GSPro (Connect window open?)" => "Warte auf GSPro (Connect-Fenster offen?)",
        "GSPro disconnected – reconnecting …" => "GSPro getrennt – verbinde neu …",
        "Connection error – reconnecting …" => "Verbindungsfehler – verbinde neu …",

        // Notifications (notify::alert)
        "R10 battery low" => "R10-Akku schwach",
        "R10 connection lost" => "Verbindung zum R10 verloren",
        "GSPro connection lost" => "Verbindung zu GSPro verloren",
        "Shot was not delivered to GSPro" => "Schlag wurde nicht an GSPro übergeben",

        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// String literals that directly follow `prefix` in `src`, e.g. every `tr("…")`.
    fn literals<'a>(src: &'a str, prefix: &str) -> Vec<&'a str> {
        let pat = format!("{prefix}\"");
        src.match_indices(&pat)
            .filter_map(|(i, _)| {
                let rest = &src[i + pat.len()..];
                rest.find("\")").map(|end| &rest[..end])
            })
            .collect()
    }

    #[test]
    fn every_ui_text_has_a_german_translation() {
        let gui = [include_str!("bin/openlaunch-connect.rs"), include_str!("bin/openlaunch-connect/explorer.rs")]
            .concat()
            .replace("\\n", "\n");
        let mut missing: Vec<&str> =
            [literals(&gui, "tr("), literals(&gui, "send_now(")].concat().into_iter().filter(|t| de(t).is_none()).collect();
        // Status details set by the bridge threads.
        let bridge = [include_str!("engine.rs"), include_str!("gspro.rs")].concat();
        for prefix in ["R10Status::Searching, ", "R10Status::Connected, ", "R10Status::Ready, ", "set_gspro(false, "] {
            missing.extend(literals(&bridge, prefix).into_iter().filter(|t| de(t).is_none()));
        }
        // Notification texts, including formatted ones like "R10 battery low ({level} %)".
        for kind in ["BatteryLow", "DeviceError", "R10Lost", "GsproLost", "ShotNotDelivered"] {
            for prefix in [format!("Alert::{kind}, "), format!("Alert::{kind}, &format!(")] {
                missing.extend(
                    literals(&bridge, &prefix)
                        .into_iter()
                        .map(|t| t.split(" (").next().unwrap_or(t))
                        .filter(|t| de(t).is_none()),
                );
            }
        }
        assert!(missing.is_empty(), "missing German texts: {missing:#?}");
    }

    #[test]
    fn texts_are_recognised_in_both_languages() {
        assert!(is_text("Ball flight", "Ball flight"));
        assert!(is_text("Ball flight", "Ballflug"));
        assert!(!is_text("Ball flight", "Range"));
    }

    #[test]
    fn details_keep_their_suffix() {
        GERMAN.store(true, Ordering::Relaxed);
        assert_eq!(tr_detail("Connected (127.0.0.1:921)"), "Verbunden (127.0.0.1:921)");
        assert_eq!(
            tr_detail("R10 is tilted – please level it (Roll 1.0°, Pitch 4.2°)"),
            "R10 steht schief – bitte gerade ausrichten (Roll 1.0°, Pitch 4.2°)"
        );
        assert_eq!(tr_detail("Radar saturated (too close to wall/net?)"), "Radar übersteuert (zu nah an Wand/Netz?)");
        assert_eq!(tr_detail("Something new"), "Something new");
        assert_eq!(tr("Not in the table"), "Not in the table");
        GERMAN.store(false, Ordering::Relaxed);
        assert_eq!(tr("SETTINGS"), "SETTINGS");
    }
}
