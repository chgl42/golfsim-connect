# Manual tests with the real setup

The unit tests cover the logic (mapping, flight, settings, history, the OpenConnect link against a fake GSPro).
What they can't reach – Bluetooth, GSPro under CrossOver, its database, macOS windows – is checked by hand on the
MacBook with the R10 and GSPro before a release, and for a PR that touches one of these areas (see *Definition of
Done* in `CLAUDE.md`).

**Setup for all of them:** the build under test installed (`./make-app.sh --deploy <host>` or the PR artifact),
iPhone Bluetooth off, R10 on its stand, GSPro installed in its CrossOver bottle. Follow the log on the *Log* page,
or start the app with `open -n --stderr /tmp/olc.log "/Applications/OpenLaunch Connect.app"` and `tail -f /tmp/olc.log`.
Each item: setup → action → what you should see (log line or screen). Lines in the log start with the time and
`[R10]` or `[GSPro]`.

## Bluetooth / R10

- [ ] **Connect.** R10 off, app running. → Switch the R10 on. → `[R10] Bluetooth connected (…)`, then
  `Handshake complete`, `Tee distance set (… m)` and `🟢 Ready – hit away`; the status panel shows *READY*.
  The first time, macOS may ask to pair – confirm it (else `No handshake from R10 after 30 s`).
- [ ] **Switch off and on.** R10 ready. → Switch it off, wait 10 s, switch it on. → At once
  `Lost connection to R10 – retrying in … s …` and *Searching*; after switching on the same connect lines as above,
  without restarting the app. A shot after that reaches GSPro.
- [ ] **Tilt error.** R10 ready. → Tip it sideways by hand for a few seconds, then put it back on its stand. →
  `⚠️  R10 is tilted – please level it (Roll …°, Pitch …°) [...]` and a notification (if on); the status shows the
  tilt. Back on its stand, after the tilt reports stop: `Standby – waking R10 …`, then `🟢 Ready – hit away`.
- [ ] **Battery.** R10 connected. → Watch the top bar and the log. → `[R10] Battery: NN %` soon after connecting,
  the battery glyph matches it. A new line only when the level changed by 5 % or more. At or below the warning level
  (red glyph) one notification *R10 battery low (NN %)*.
- [ ] **iPhone takes the R10.** iPhone Bluetooth on with the Garmin app open. → Start the app. →
  `No R10 connected: …` / `Retrying in … s …` and the hint to turn off iPhone Bluetooth; after turning it off the
  app connects by itself.

## GSPro link

- [ ] **Start order: app first.** GSPro not running, app running. → Start GSPro, open its Connect window, tee up a
  ball on the range. → `Waiting for GSPro OpenConnect at 127.0.0.1:921 (…)` once, then
  `✅ Connected to GSPro OpenConnect (127.0.0.1:921)`; GSPro's Connect window shows the launch monitor as ready.
- [ ] **Start order: GSPro first.** GSPro on the range. → Start the app. → Connected within a few seconds; the
  first shot flies.
- [ ] **A shot.** Both connected, ball teed up, GSPro in front. → Hit a shot. → `[R10] ⛳ Shot #…: ball … mph | …`,
  `[GSPro] Sending shot to GSPro (ShotNumber …) …`, `GSPro acknowledged the shot (…)`; the ball flies in GSPro and
  the shot appears on the main page and the *Shots* page.
- [ ] **Club from GSPro.** → Change the club in GSPro. → `[GSPro] Player: RH, club: I7` (your hand and club);
  the club tile shows it, the next shot is stored with that club.
- [ ] **GSPro restart.** Connected. → Quit GSPro, wait, start it again and go to the range. →
  `GSPro closed the connection – reconnecting …` (or `Lost connection to GSPro (…)`), the GSPro status turns red,
  a notification *GSPro connection lost*; after the restart `✅ Connected …` again, without touching the app.
- [ ] **Held shot.** Connected. → Quit GSPro, hit a shot, start GSPro again within 10 s. →
  `GSPro not connected – holding the shot for 10 s …`, then `GSPro is back – sending the held shot (… s late).`
  Hit one while GSPro stays away: `⚠️  Shot dropped – GSPro didn't come back within 10 s.` and the shot is marked
  as not delivered.
- [ ] **Reset GSPro Connect hint.** Make GSPro Connect stop answering (e.g. start the app while GSPro still holds
  the connection of an earlier run). → Hit two shots. → `No direct reply from GSPro to the shot.` twice, then
  `⚠️  GSPro Connect doesn't answer – press Reset GSPro Connect in GSPro's settings.` and the same text as the GSPro
  status. → Press *Reset GSPro Connect* in GSPro. → `GSPro Connect answers again.` after the next shot (or a new
  connection), the status is *Connected* again.
- [ ] **Test shot.** → *TEST SHOT IN 3 S*, switch to GSPro. → `Test shot in 3 s – switch to the GSPro window now …`,
  the ball flies; the test shot is not in the shot history.

## GSPro's results (GSPro.db)

- [ ] **Carry, total, to pin.** *GSPro results* chip on (on the *Shots* page), playing a hole on a course. → Hit a real
  shot and wait until the ball has come to rest. → `[GSPro] GSPro result: carry … m, total … m, apex … m,
  offline … m, … m to the pin`; the numbers equal GSPro's own shot display (in yards when GSPro shows yards – the
  app converts); the overlay's pin tile shows the distance left.
- [ ] **Kept with the shot.** → Open the *Shots* page, then restart the app. → The shot's carry/total are GSPro's
  (not the estimate) before and after the restart.
- [ ] **Test shot result.** → A test shot on the range. → `GSPro result of a shot not in the session (e.g. a test
  shot): …`; nothing changes in the history.
- [ ] **Range.** → A shot on the range (no pin). → A result line without "to the pin".

## Overlays

- [ ] **In front.** Overlay on (Overlay page), GSPro in front. → Hit a shot. → The status overlay shows READY,
  club and the shot's values; clicks go through it to GSPro.
- [ ] **Behind other apps.** → Bring another app (e.g. Finder) in front of GSPro. → The overlays sit right under
  that window, above GSPro – not on top of everything; with GSPro quit they are hidden.
- [ ] **Second screen.** GSPro on the second screen (projector), the app's window on the first. → Hit a shot. →
  The overlays stay on GSPro's screen, at the chosen corner, wherever the main window is.
- [ ] **Full screen.** → Put GSPro in full screen. → The overlays stay visible on top of it (issue #2: report
  what you see).
- [ ] **Moving.** → *Move on screen* on the Overlay page, drag the status and a tile overlay into gaps of GSPro's
  display, let go. → They stay there, also after a restart of the app and of GSPro, and let clicks through again.
- [ ] **Arrangements.** → Add a tile overlay, save the overlays as a new arrangement, switch between the two. →
  Each arrangement brings back its own overlays (place, size, tiles); after a restart the one in use is still in use.
- [ ] **GSPro's menus.** A tile overlay on. → Start GSPro (main menu). → Only the status overlay shows; the log
  says *GSPro is in: Menu*. → Open the range. → The tile overlays come back (*GSPro is in: Range*). → Back to the
  menu, then load a course. → Hidden in the menu, back on the course (*Course*). The log shows no player name.

## Settings and history survive a restart

- [ ] **Settings.** → Change a few settings (units, language, interface size, notifications off, an overlay
  position, a page on the *Shots* page), quit with ⌘Q and start again. → Everything as you left it; the window size
  too.
- [ ] **History.** → Hit a few shots, exclude one on the *Shots* page, quit, start again. → All shots are there,
  the excluded one still excluded; an export (CSV) contains the same shots without it.
- [ ] **Update from the last release.** → Install the last release, use it, then install the build under test over
  it. → Settings and history carry over unchanged (nothing falls back to the defaults).
