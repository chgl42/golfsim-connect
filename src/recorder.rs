//! The core's record of what the bridge did: a shot handed over by the GSPro link, and GSPro's
//! result of a shot. Writes the shared state (last shot, session), the shot history and the
//! notification, then publishes the [`Event`] – so the protocol code only says what happened.
//!
//! It is also the one way to change a recorded shot ([`change_shot`], ADR-015): the history file,
//! then the session, then [`Event::ShotChanged`].
//!
//! Called directly (not as a subscriber), so the state is written before the caller goes on: the
//! GUI and the next shot see it at once, and a `ShotRecorded` is always published before the
//! `GsproResult` of that shot.

use std::time::Instant;

use crate::events::{self, Event};
use crate::history;
use crate::log;
use crate::mapping::{GsBall, GsClub, Tempo};
use crate::misread;
use crate::notify::{self, Alert};
use crate::state;

/// Record a shot the GSPro link sent (`delivered`) or gave up. It becomes the last shot and,
/// unless it is a test shot, joins the session and the history with the club chosen in GSPro.
pub fn record_shot(ball: GsBall, club: Option<GsClub>, tempo: Option<Tempo>, delivered: bool, shot_number: u32, test: bool) {
    let misread = misread::check(&ball, club.as_ref());
    if let Some(m) = misread {
        log::gspro(&format!("⚠️  Possible misread ({}) – the shot is left out of the stats.", m.reason()));
    }
    let rec = state::with(|s| {
        if delivered {
            s.shots_sent += 1;
        }
        s.last_shot = Some(state::LastShot { ball, club, tempo, at: Instant::now(), delivered, shot_number, misread });
        // GSPro's result of the previous shot no longer applies (the new one follows when it lands).
        s.gspro_result = None;
        let mut rec = history::ShotRecord::new(ball, club, s.gspro_club.clone(), delivered, shot_number);
        rec.tempo = tempo;
        rec.misread = misread;
        rec.excluded = misread.is_some();
        if !test {
            s.session.push(rec.clone());
        }
        rec
    });
    if !delivered && !test {
        notify::alert(Alert::ShotNotDelivered, "Shot was not delivered to GSPro");
    }
    if !test
        && let Err(e) = history::append(&rec)
    {
        log::gspro(&format!("⚠️  Could not save shot history: {e}"));
    }
    events::publish(Event::ShotRecorded { shot: Box::new(rec), test });
}

/// Keep GSPro's result of a shot: on the session shot with this time and number (and in the
/// history), or – for a shot not in the session (`None`, e.g. a test shot) – only as the latest
/// result.
pub fn gspro_result(shot: Option<(String, u32)>, result: crate::gspro_db::GsproResult) {
    state::with(|s| {
        if let Some((time, number)) = &shot
            && let Some(r) = s.session.iter_mut().rev().find(|r| (&r.time, r.shot_number) == (time, *number))
        {
            r.gspro = Some(result);
        }
        s.gspro_result = Some(result);
    });
    if let Some((time, number)) = &shot
        && let Err(e) = history::set_gspro(time, *number, result)
    {
        log::gspro(&format!("Could not save GSPro's result: {e}"));
    }
    events::publish(Event::GsproResult { shot, result });
}

/// A change to a recorded shot, made with [`change_shot`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShotChange {
    /// Leave the shot out of the stats and exports (`true`) or count it again (`false`).
    Excluded(bool),
    /// Count the shot for this club (a GSPro club code, e.g. `I7`): the club chosen in GSPro was
    /// the wrong one. GSPro's club is kept in `club_gspro` until the shot is corrected back to it.
    Club(String),
}

impl ShotChange {
    /// Makes the change on `rec`; nothing but `excluded`, or `club_code` and `club_gspro`, changes.
    fn apply(&self, rec: &mut history::ShotRecord) {
        match self {
            Self::Excluded(excluded) => rec.excluded = *excluded,
            Self::Club(code) => {
                if rec.club_code.as_ref() == Some(code) {
                    return;
                }
                if rec.club_gspro.is_none() {
                    // The first correction: what GSPro sent ("" for no club).
                    rec.club_gspro = Some(rec.club_code.clone().unwrap_or_default());
                }
                rec.club_code = Some(code.clone());
                if rec.club_gspro.as_ref() == Some(code) {
                    rec.club_gspro = None;
                }
            }
        }
    }
}

/// Held across the whole of [`change_shot`], so changes from several threads write the file and
/// publish their events in the same order.
static CHANGES: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Changes the recorded shot with this time and number – the one way to change a shot: first
/// `shots.jsonl` (not in demo mode), then the session (if the shot is in it) and
/// [`state::State::shot_changes`], then [`Event::ShotChanged`] with the record as it is now, which
/// is returned. A change that makes no difference writes nothing and publishes nothing. Changes
/// are made one at a time, so their events come in the order the file was written.
///
/// # Errors
/// Returns a message if the club code is empty, if there is no such shot (in the history; in demo
/// mode, in the session) or the history file cannot be read or written; then nothing has changed.
pub fn change_shot(time: &str, shot_number: u32, change: &ShotChange) -> Result<history::ShotRecord, String> {
    if matches!(change, ShotChange::Club(code) if code.trim().is_empty()) {
        return Err("no club given".into());
    }
    let _one_at_a_time = CHANGES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let missing = || format!("no shot recorded at {time} with number {shot_number}");
    let (rec, changed) = if history::persist() {
        history::update(time, shot_number, |rec| change.apply(rec))?.ok_or_else(missing)?
    } else {
        // Demo mode: the session is all there is.
        let mut rec = state::with(|s| s.session.iter().find(|r| (r.time.as_str(), r.shot_number) == (time, shot_number)).cloned()).ok_or_else(missing)?;
        let before = rec.clone();
        change.apply(&mut rec);
        let changed = rec != before;
        (rec, changed)
    };
    if !changed {
        return Ok(rec);
    }
    state::with(|s| {
        for r in s.session.iter_mut().filter(|r| (r.time.as_str(), r.shot_number) == (time, shot_number)) {
            change.apply(r);
        }
        s.shot_changes += 1;
    });
    events::publish(Event::ShotChanged { shot: Box::new(rec.clone()) });
    Ok(rec)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `record_shot`: the shot becomes the last one and, unless it is a test shot, joins the session
    /// and the history, with the club GSPro has chosen; GSPro's result of the shot before is gone.
    #[test]
    fn recorded_shots_go_to_the_session_and_the_history() {
        let _state = crate::test_util::state_lock();
        let _history = crate::test_util::TestHistory::new("record-shot");
        notify::ENABLED.store(false, std::sync::atomic::Ordering::Relaxed);
        let earlier = crate::gspro_db::GsproResult { carry: 1.0, total: 2.0, side: 0.0, apex: 1.0, descent: 1.0, to_pin: None };
        let (sent, session) = state::with(|s| {
            s.gspro_club = Some("PW".into());
            s.gspro_result = Some(earlier);
            (s.shots_sent, s.session.len())
        });
        let (ball, club) = crate::engine::test_shot();
        let tempo = Some(Tempo { backswing: 0.8, downswing: 0.27 });
        record_shot(ball, Some(club), tempo, true, 4711, false);
        let (last, now_sent, rec, result) = state::with(|s| {
            (s.last_shot.clone().unwrap(), s.shots_sent, s.session.last().cloned().unwrap(), s.gspro_result)
        });
        assert_eq!((last.ball, last.club, last.tempo, last.delivered, last.shot_number), (ball, Some(club), tempo, true, 4711));
        assert_eq!(last.misread, None);
        assert_eq!(now_sent, sent + 1);
        assert_eq!(result, None, "the earlier shot's result no longer applies");
        assert_eq!((rec.ball, rec.club, rec.tempo, rec.shot_number), (ball, Some(club), tempo, 4711));
        assert_eq!((rec.club_code.as_deref(), rec.delivered, rec.excluded), (Some("PW"), true, false));
        assert_eq!(history::load_all(), std::slice::from_ref(&rec));

        // A test shot: the last shot, but neither in the session nor in the history.
        let (test_ball, _) = crate::engine::test_shot();
        record_shot(test_ball, None, None, true, 4712, true);
        let (last, n, now_sent) = state::with(|s| (s.last_shot.clone().unwrap(), s.session.len(), s.shots_sent));
        assert_eq!((last.ball, last.shot_number), (test_ball, 4712));
        assert_eq!((n, now_sent), (session + 1, sent + 2), "counted as sent, not kept");
        assert_eq!(history::load_all().len(), 1);

        // Not delivered: kept, not counted as sent. A misread is kept but excluded.
        let misread = GsBall { total_spin: 100.0, ..ball };
        record_shot(misread, Some(club), None, false, 0, false);
        let (last, rec, now_sent) = state::with(|s| (s.last_shot.clone().unwrap(), s.session.last().cloned().unwrap(), s.shots_sent));
        assert!(!last.delivered && last.misread.is_some());
        assert!(!rec.delivered && rec.excluded && rec.misread.is_some());
        assert_eq!(now_sent, sent + 2);
        assert_eq!(history::load_all().len(), 2);
    }


    /// Recording a shot and keeping GSPro's result publish their events, after the state is written.
    #[test]
    fn recording_publishes_events() {
        let _state = crate::test_util::state_lock();
        let _history = crate::test_util::TestHistory::new("recorder-events");
        notify::ENABLED.store(false, std::sync::atomic::Ordering::Relaxed);
        let rx = events::subscribe();
        let (ball, club) = crate::engine::test_shot();
        record_shot(ball, Some(club), None, true, 815, false);
        let (test_ball, _) = crate::engine::test_shot();
        record_shot(test_ball, None, None, false, 816, true);
        let key = state::with(|s| s.session.last().map(|r| (r.time.clone(), r.shot_number))).unwrap();
        let result = crate::gspro_db::GsproResult { carry: 150.0, total: 160.0, side: 2.0, apex: 28.0, descent: 45.0, to_pin: None };
        gspro_result(Some(key.clone()), result);
        gspro_result(None, result);

        let mine: Vec<Event> = rx
            .try_iter()
            .filter(|e| match e {
                Event::ShotRecorded { shot, .. } => [815, 816].contains(&shot.shot_number),
                Event::GsproResult { shot, .. } => shot.as_ref().is_none_or(|k| k == &key),
                _ => false,
            })
            .collect();
        let [Event::ShotRecorded { shot: first, test: false }, Event::ShotRecorded { shot: second, test: true }, Event::GsproResult { shot: Some(k), result: r1 }, Event::GsproResult { shot: None, result: r2 }] =
            mine.as_slice()
        else {
            panic!("unexpected events: {mine:?}");
        };
        assert_eq!((first.ball, first.delivered, first.shot_number), (ball, true, 815));
        assert_eq!((second.ball, second.delivered), (test_ball, false));
        assert_eq!((k, *r1, *r2), (&key, result, result));
        assert_eq!(state::with(|s| s.session.last().and_then(|r| r.gspro)), Some(result), "stored before it was published");
        assert_eq!(history::load_all().last().and_then(|r| r.gspro), Some(result));
    }

    /// A shot of `club` with this number, at a time of its own.
    fn shot(number: u32, club: &str) -> history::ShotRecord {
        let (ball, gs_club) = crate::engine::test_shot();
        let mut rec = history::ShotRecord::new(ball, Some(gs_club), Some(club.into()), true, number);
        rec.time = format!("2026-11-08T10:{:02}:{:02}+01:00", number / 60 % 60, number % 60);
        rec
    }

    /// The `ShotChanged` events of these shot numbers.
    fn changed(rx: &std::sync::mpsc::Receiver<Event>, numbers: &[u32]) -> Vec<history::ShotRecord> {
        rx.try_iter()
            .filter_map(|e| match e {
                Event::ShotChanged { shot } if numbers.contains(&shot.shot_number) => Some(*shot),
                _ => None,
            })
            .collect()
    }

    /// The session shot with this number.
    fn in_session(number: u32) -> Option<history::ShotRecord> {
        state::with(|s| s.session.iter().find(|r| r.shot_number == number).cloned())
    }

    /// Correcting a club: the file and the session count the shot for the new club, GSPro's is
    /// kept, one `ShotChanged` carries the new record and the CSV uses the new club; corrected
    /// back, GSPro's is cleared; the same club again changes nothing.
    #[test]
    fn change_shot_corrects_a_club() {
        let _state = crate::test_util::state_lock();
        let _history = crate::test_util::TestHistory::new("change-shot-club");
        let rx = events::subscribe();
        let rec = shot(901, "I6");
        history::append(&rec).unwrap();
        state::with(|s| s.session.push(rec.clone()));
        let changes = state::with(|s| s.shot_changes);

        let now = change_shot(&rec.time, 901, &ShotChange::Club("I7".into())).unwrap();
        assert_eq!((now.club_code.as_deref(), now.club_gspro.as_deref()), (Some("I7"), Some("I6")));
        let want = history::ShotRecord { club_code: Some("I7".into()), club_gspro: Some("I6".into()), ..rec.clone() };
        assert_eq!(now, want, "only the club changes");
        assert_eq!(history::load_all(), std::slice::from_ref(&want));
        assert_eq!(in_session(901), Some(want.clone()));
        assert_eq!(state::with(|s| s.shot_changes), changes + 1);
        assert_eq!(changed(&rx, &[901]), std::slice::from_ref(&want));
        assert!(history::to_csv(&history::load_all()).lines().nth(1).unwrap().contains(",901,I7,"));

        // The same club again: nothing written, no event.
        assert_eq!(change_shot(&rec.time, 901, &ShotChange::Club("I7".into())), Ok(want));
        assert!(changed(&rx, &[901]).is_empty());
        assert_eq!(state::with(|s| s.shot_changes), changes + 1);

        // Corrected on to another club, then back: GSPro's stays, then is cleared.
        let on = change_shot(&rec.time, 901, &ShotChange::Club("I8".into())).unwrap();
        assert_eq!((on.club_code.as_deref(), on.club_gspro.as_deref()), (Some("I8"), Some("I6")));
        let back = change_shot(&rec.time, 901, &ShotChange::Club("I6".into())).unwrap();
        assert_eq!(back, rec, "as GSPro sent it");
        assert_eq!(history::load_all(), std::slice::from_ref(&rec));
        assert_eq!(in_session(901), Some(rec));
        assert_eq!(changed(&rx, &[901]).len(), 2);
        state::with(|s| s.session.retain(|r| r.shot_number != 901));
    }

    /// Excluding through the API: file and session, one event per shot; a shot of an earlier day
    /// (not in the session) changes in the file only, and its event is published.
    #[test]
    fn change_shot_excludes_in_the_file_and_the_session() {
        let _state = crate::test_util::state_lock();
        let _history = crate::test_util::TestHistory::new("change-shot-exclude");
        let rx = events::subscribe();
        let (earlier, today, other) = (shot(911, "DR"), shot(912, "PW"), shot(913, "PW"));
        for r in [&earlier, &today, &other] {
            history::append(r).unwrap();
        }
        state::with(|s| s.session.extend([today.clone(), other.clone()]));
        for r in [&earlier, &today] {
            change_shot(&r.time, r.shot_number, &ShotChange::Excluded(true)).unwrap();
        }
        let excluded = |r: &history::ShotRecord| history::ShotRecord { excluded: true, ..r.clone() };
        assert_eq!(history::load_all(), [excluded(&earlier), excluded(&today), other.clone()]);
        assert_eq!((in_session(911), in_session(912), in_session(913)), (None, Some(excluded(&today)), Some(other)));
        assert_eq!(changed(&rx, &[911, 912, 913]), [excluded(&earlier), excluded(&today)]);
        state::with(|s| s.session.retain(|r| ![912, 913].contains(&r.shot_number)));
    }

    /// No such shot, or a history that can't be read: an error, nothing changed, no event.
    #[test]
    fn change_shot_changes_nothing_on_an_error() {
        let _state = crate::test_util::state_lock();
        let history = crate::test_util::TestHistory::new("change-shot-error");
        let rx = events::subscribe();
        let rec = shot(921, "I6");
        state::with(|s| s.session.push(rec.clone()));
        let changes = state::with(|s| s.shot_changes);

        // No file yet: it can't be read.
        assert!(change_shot(&rec.time, 921, &ShotChange::Club("I7".into())).is_err());
        assert_eq!(in_session(921), Some(rec.clone()), "memory unchanged");
        // In the file, but not with that key.
        history::append(&rec).unwrap();
        let err = change_shot(&rec.time, 922, &ShotChange::Excluded(true)).unwrap_err();
        assert!(err.contains("922"), "{err}");
        assert_eq!(history.text().lines().count(), 1);
        assert_eq!(history::load_all(), std::slice::from_ref(&rec));
        assert_eq!(in_session(921), Some(rec));
        assert_eq!(state::with(|s| s.shot_changes), changes);
        assert!(changed(&rx, &[921, 922]).is_empty());
        state::with(|s| s.session.retain(|r| r.shot_number != 921));
    }

    /// Demo mode (nothing saved): the session changes and the event is published, no file is
    /// written; a shot not in the session is an error.
    #[test]
    fn change_shot_in_demo_mode_changes_the_session() {
        let _state = crate::test_util::state_lock();
        // No `TestHistory`: this thread writes no history, as with `PERSIST` off.
        let rx = events::subscribe();
        let rec = shot(931, "I6");
        state::with(|s| s.session.push(rec.clone()));
        let now = change_shot(&rec.time, 931, &ShotChange::Club("I7".into())).unwrap();
        assert_eq!(in_session(931), Some(now.clone()));
        assert_eq!(now.club_gspro.as_deref(), Some("I6"));
        assert_eq!(changed(&rx, &[931]), [now]);
        assert!(change_shot(&rec.time, 932, &ShotChange::Excluded(true)).is_err());
        state::with(|s| s.session.retain(|r| r.shot_number != 931));
    }

    /// An empty or blank club code is refused: nothing written, no event.
    #[test]
    fn change_shot_refuses_an_empty_club() {
        let _state = crate::test_util::state_lock();
        let history = crate::test_util::TestHistory::new("change-shot-empty");
        let rx = events::subscribe();
        let rec = shot(971, "I6");
        history::append(&rec).unwrap();
        let text = history.text();
        for code in ["", "  "] {
            assert!(change_shot(&rec.time, 971, &ShotChange::Club(code.into())).is_err(), "{code:?}");
        }
        assert_eq!(history.text(), text, "not rewritten");
        assert!(changed(&rx, &[971]).is_empty());
    }

    /// Two threads changing one shot: the last `ShotChanged` is the record in the file and the
    /// session, so a subscriber never ends on an older one.
    #[test]
    fn change_shot_publishes_in_file_order_across_threads() {
        let _state = crate::test_util::state_lock();
        let history = crate::test_util::TestHistory::new("change-shot-threads");
        let rx = events::subscribe();
        let rec = shot(981, "I6");
        history::append(&rec).unwrap();
        state::with(|s| s.session.push(rec.clone()));
        let threads: Vec<_> = [["I7", "I8"], ["I9", "PW"]]
            .into_iter()
            .map(|codes| {
                let (file, time) = (history.file.clone(), rec.time.clone());
                std::thread::spawn(move || {
                    history::TEST_FILE.set(Some(file));
                    for i in 0..40 {
                        change_shot(&time, 981, &ShotChange::Club(codes[i % 2].into())).unwrap();
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        let last = changed(&rx, &[981]).pop().unwrap();
        assert_eq!(history::load_all(), std::slice::from_ref(&last));
        assert_eq!(in_session(981), Some(last));
        state::with(|s| s.session.retain(|r| r.shot_number != 981));
    }

    /// A shot GSPro sent without a club keeps that as `""`, so a second correction doesn't take
    /// the first one for GSPro's.
    #[test]
    fn change_shot_keeps_that_gspro_sent_no_club() {
        let mut rec = shot(941, "I6");
        rec.club_code = None;
        ShotChange::Club("I7".into()).apply(&mut rec);
        ShotChange::Club("I8".into()).apply(&mut rec);
        assert_eq!((rec.club_code.as_deref(), rec.club_gspro.as_deref()), (Some("I8"), Some("")));
    }
}
