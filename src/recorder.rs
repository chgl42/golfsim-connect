//! The core's record of what the bridge did: a shot handed over by the GSPro link, and GSPro's
//! result of a shot. Writes the shared state (last shot, session), the shot history and the
//! notification, then publishes the [`Event`] – so the protocol code only says what happened.
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
}
