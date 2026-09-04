use crate::support::flutter::{click_center, first_matching, text_matching_within, type_into, type_into_verified};
use anyhow::{anyhow, Result};
use playwright_rs::expect;
use playwright_rs::protocol::locator::{AriaRole, GetByRoleOptions};
use playwright_rs::protocol::page::Page;
use regex::Regex;
use std::time::Duration;

// The task list defaults to an unfiltered, paginated view ordered in a way that doesn't
// prioritize today or "just booked" -- leftover appointments from past runs, dated days
// apart, can fill the entire visible first page while a same-day appointment booked moments
// earlier is nowhere on it. Filtering via the list's own search box narrows it down to this
// patient specifically, sidestepping that pagination/ordering entirely.
//
// Entering an appointment that hasn't been opened before shows a Symptoms/Allergies/Vitals
// intake dialog first; one already entered skips straight to the consultation screen. Both
// land on the same /appointmentHostHAdmin screen, so handle either.
//
// Matches on patient name, the booked date, AND the booked time (e.g. "1/9/2026" and
// "9:00 AM", as returned by book_appointment) together, not name alone -- on a reused fixed
// patient like EXISTING_PATIENT_NAME, a name-only match always lands on whichever "Brad Pitt"
// row comes first in the task list, which is that patient's *oldest* appointment, not the one
// this run just booked. Confirmed live: that oldest appointment accumulates medicines/imaging
// orders added by every past run that entered it, so re-entering it instead of the fresh one
// this run booked causes spurious "already added" failures (e.g. the medicine autocomplete no
// longer suggesting a medicine that's already on that stale appointment).
//
// Date matters just as much as time here: this suite's own repeated runs routinely exhaust
// today's bookable slots, pushing book_appointment to a future day -- and that day's time slot
// (e.g. "9:00 AM") can easily coincide with an old stale appointment's time on an *earlier*
// day. Matching on time alone (confirmed live) can silently re-enter that wrong, stale
// appointment instead of erroring, since both rows satisfy a time-only pattern.
//
// Row text observed live puts the patient name before the date, and the date before the time
// ("Brad Pitt Dr. Kishan D 1/9/2026 , 9:00 AM ..."), so requiring all three in that order in
// the same match is reliable.
pub async fn enter_appointment(page: &Page, patient_name: &str, date_label: &str, slot_label: &str) -> Result<()> {
    let task_search_box = page
        .get_by_role(AriaRole::Textbox, Some(GetByRoleOptions::default().name("Search tasks by patient name").exact(false)))
        .first();
    expect(task_search_box.clone()).with_timeout(Duration::from_secs(15)).to_be_visible().await?;
    // This runs right after book_appointment just booked a fresh slot and closed its dialog --
    // give the task list a moment to finish settling before interacting with it.
    tokio::time::sleep(Duration::from_millis(800)).await;
    // (?s) so `.` also matches a newline: a row's accessible name isn't always a single space-
    // joined string -- when first_matching falls back from an empty aria-label to inner_text(),
    // that can put each cell on its own line, which would otherwise break a pattern spanning
    // from the name cell to the date/time cell.
    let row_pattern = Regex::new(&format!(
        "(?is){}.*{}.*{}",
        regex::escape(patient_name),
        regex::escape(date_label),
        regex::escape(slot_label)
    ))
    .unwrap();
    // This search box's accessible name/state can change once it has a match (unlike the other
    // fields in this flow), which makes type_into_verified's input_value() check unreliable
    // here specifically -- a spurious "didn't match" retry then fails to re-find the field at
    // all. Instead retry against the signal that actually matters: the target row showing up.
    // If typing silently didn't land (the same class of dropped-input race seen elsewhere in
    // this flow) a fresh click+retype recovers it without depending on input_value().
    let mut row = None;
    for attempt in 1..=4 {
        if attempt > 1 {
            eprintln!("[consult] row not found yet, retyping task search (attempt {attempt})");
            // This whole page can still be settling right after book_appointment just closed
            // its dialog and returned -- a click here can transiently fail with the search box
            // "not visible" rather than actually being gone for good. Wait and retry the loop
            // rather than let one such click failure abort the entire function.
            if let Err(e) = click_center(page, &task_search_box).await {
                eprintln!("[consult] search box click failed, waiting and retrying: {e}");
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
            page.keyboard().press("Control+A", None).await?;
            page.keyboard().press("Backspace", None).await?;
        }
        type_into(page, &task_search_box, patient_name).await?;
        eprintln!("[consult] typed patient name into task search");
        row = first_matching_with_timeout(page.get_by_role(AriaRole::Row, None), &row_pattern, Duration::from_secs(15)).await.ok();
        if row.is_none() {
            // This is a standard paginated table (confirmed live: 10 rows per page, with
            // First/Previous/Next/Last controls) -- with enough stale appointments piled up
            // for this reused test patient across repeated suite runs (see comment above), a
            // freshly booked row routinely lands on a later page that a name-only search
            // filter doesn't reveal on its own. No amount of retyping the same search text
            // fixes that. Page forward and rescan after each page before concluding it's
            // genuinely not there.
            let next_page_button =
                page.get_by_role(AriaRole::Button, Some(GetByRoleOptions::default().name("Next page").exact(true))).first();
            for _ in 0..20 {
                if !next_page_button.is_enabled().await.unwrap_or(false) {
                    break;
                }
                click_center(page, &next_page_button).await?;
                tokio::time::sleep(Duration::from_millis(400)).await;
                row = first_matching_with_timeout(page.get_by_role(AriaRole::Row, None), &row_pattern, Duration::from_secs(5)).await.ok();
                if row.is_some() {
                    break;
                }
            }
        }
        if row.is_some() {
            break;
        }
    }
    let row = row.ok_or_else(|| anyhow!("Could not find anything matching {row_pattern} after retrying the task search."))?;
    eprintln!("[consult] found matching row");
    let open_button = row.get_by_role(AriaRole::Button, None).nth(1);
    click_center(page, &open_button).await?;
    eprintln!("[consult] clicked open button");

    let symptoms_input = page
        .get_by_role(AriaRole::Textbox, Some(GetByRoleOptions::default().name("Enter symptoms").exact(false)))
        .first();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    let mut shown = false;
    while tokio::time::Instant::now() < deadline {
        if symptoms_input.is_visible().await.unwrap_or(false) {
            shown = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    eprintln!("[consult] symptoms shown inline: {shown}");
    if shown {
        // This is a plain inline field on the main consultation page, not a separate modal
        // with its own Save/Cancel -- there is no dedicated "Save" for it at all. Confirmed
        // live: this page's *only* exact-match "Save" button is the prescription form's, way
        // down at the bottom. An earlier version of this function clicked that unscoped "Save"
        // right after typing here, which (confirmed live) silently submitted the prescription
        // form empty and popped up a "Write Prescription: Please type the prescription
        // details..." validation dialog that then sat blocking the whole page -- including the
        // Medications toggle, which is why that toggle intermittently seemed impossible to
        // click no matter how long add_medicine retried. Just type the symptom text and move
        // on; it's picked up along with everything else when the real prescription Save runs
        // later in save_and_finish_appointment.
        type_into(page, &symptoms_input, "cough").await?;
    }

    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    while !page.url().contains("appointmentHostHAdmin") {
        if tokio::time::Instant::now() >= deadline {
            return Err(anyhow!("Never navigated to appointmentHostHAdmin. Last URL: {}", page.url()));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    eprintln!("[consult] on appointmentHostHAdmin");
    Ok(())
}

async fn first_matching_with_timeout(
    candidates: playwright_rs::protocol::locator::Locator,
    pattern: &Regex,
    timeout: Duration,
) -> Result<playwright_rs::protocol::locator::Locator> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if let Some(found) = first_matching(&candidates, pattern).await? {
            return Ok(found);
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(anyhow!("Could not find anything matching {pattern} within {timeout:?}."));
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

// The medicine field is an autocomplete: typing free text alone is rejected ("Please select
// medicine") -- a suggestion has to actually be clicked. The exact suggestion list varies
// (e.g. "Dolo 500 Tablet" vs "Dolo Drops" depending on what's in stock), so match whichever
// suggestion starts with the search term rather than a hardcoded full name. Adding also
// requires dosage or instructions to be filled in ("Please add dosage or instructions"
// otherwise).
pub async fn add_medicine(page: &Page, search_term: &str) -> Result<()> {
    let medications_toggle = page.get_by_role(AriaRole::Button, Some(GetByRoleOptions::default().name("Medications").exact(true))).first();
    // Root cause of this toggle's earlier flakiness (confirmed live): an unscoped "Save" click
    // in enter_appointment's symptoms-dialog handling could hit this page's own prescription
    // Save button instead, popping up a "Write Prescription" validation dialog that then
    // covers the whole page -- fixed at the source in enter_appointment. Dismiss it here too as
    // a defensive fallback in case it (or something similarly blocking) shows up for any other
    // reason, then retry the click a few times for ordinary render-timing slack.
    let write_prescription_ok = page.get_by_role(AriaRole::Button, Some(GetByRoleOptions::default().name("Ok").exact(true))).first();
    let mut last_err = None;
    for attempt in 1..=5 {
        if write_prescription_ok.is_visible().await.unwrap_or(false) {
            eprintln!("[consult] dismissing an unexpected blocking dialog before retrying");
            click_center(page, &write_prescription_ok).await?;
        }
        match click_center(page, &medications_toggle).await {
            Ok(()) => {
                last_err = None;
                break;
            }
            Err(e) => {
                eprintln!("[consult] Medications toggle click failed (attempt {attempt}), retrying: {e}");
                last_err = Some(e);
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
    if let Some(e) = last_err {
        return Err(e);
    }
    eprintln!("[consult] clicked Medications toggle");

    let medicine_input = page
        .get_by_role(AriaRole::Textbox, Some(GetByRoleOptions::default().name("eg. Ascoril").exact(false)))
        .first();
    type_into(page, &medicine_input, search_term).await?;
    eprintln!("[consult] typed medicine search term");

    // Non-tablet formulations (Infusion, Injection, Drops, ...) render a completely different
    // dosage UI (a Morning/Afternoon/Evening/Night frequency picker instead of a plain
    // Dosage/Instructions text-field pair) and are more likely to be out of stock -- confirmed
    // live via screenshot: picking an out-of-stock "Infusion" suggestion left the Instructions
    // field empty (the reflow into the frequency UI apparently dropped what was typed), which
    // then failed with "Please add dosage or instructions." Prefer a Tablet suggestion, which
    // consistently uses the simple layout this form-filling logic actually targets.
    let tablet_pattern = Regex::new(&format!("(?i)^{}.*Tablet", regex::escape(search_term))).unwrap();
    let suggestion_pattern = Regex::new(&format!("(?i)^{}", regex::escape(search_term))).unwrap();
    let suggestion = match first_matching_with_timeout(page.get_by_role(AriaRole::Button, None), &tablet_pattern, Duration::from_secs(3)).await {
        Ok(s) => s,
        Err(_) => first_matching_with_timeout(page.get_by_role(AriaRole::Button, None), &suggestion_pattern, Duration::from_secs(10)).await?,
    };
    eprintln!("[consult] found medicine suggestion");
    click_center(page, &suggestion).await?;
    eprintln!("[consult] clicked medicine suggestion");

    // Selecting a suggestion re-renders the dosage/instructions fields, so the first click
    // can land before the new field is actually interactive -- and if the chosen suggestion
    // turns out to be flagged unavailable in the pharmacy, an extra async warning line
    // ("No stock available...") reflows the form again a moment later (confirmed live via
    // screenshot: typing right after the field appeared landed correctly, but a follow-up
    // verify-and-retry click then failed because that later reflow had shifted things again
    // mid-retry). Rather than react after the fact, wait for that warning to either show up
    // or definitely not be coming before typing at all, so there's no reflow left to race.
    let instructions_input = page
        .get_by_role(AriaRole::Textbox, Some(GetByRoleOptions::default().name("Take after food").exact(false)))
        .first();
    expect(instructions_input.clone()).with_timeout(Duration::from_secs(10)).to_be_visible().await?;
    eprintln!("[consult] instructions field visible");
    let stock_warning = text_matching_within(page, &Regex::new(r"(?i)no stock available").unwrap(), Duration::from_secs(2)).await;
    eprintln!("[consult] stock warning check done: {}", stock_warning.is_ok());
    if stock_warning.is_ok() {
        // The warning banner's arrival re-renders the dosage/instructions section (it swaps
        // in a frequency-based Morning/Afternoon/Evening/Night dosage picker in place of the
        // plain field pair) -- during that swap the placeholder-derived accessible name this
        // locator matches on ("Take after food...") can briefly disappear entirely (confirmed
        // live: a re-check right after this reflow found nothing visible at all), not just
        // shift to a different node. Give the swap time to finish, then re-find the field
        // positionally by its "Instructions" label -- which is present in both the normal and
        // out-of-stock layouts -- rather than by a name that isn't reliably present mid-reflow.
        tokio::time::sleep(Duration::from_millis(1500)).await;
        let instructions_input = crate::support::flutter::textbox_near_label(page, &Regex::new(r"(?i)^Instructions$").unwrap()).await?;
        type_into_verified(page, &instructions_input, "After food").await?;
    } else {
        type_into_verified(page, &instructions_input, "After food").await?;
    }
    eprintln!("[consult] typed instructions");

    let add_med_button = page.get_by_role(AriaRole::Button, Some(GetByRoleOptions::default().name("Add Med").exact(true))).first();
    add_med_button.scroll_into_view_if_needed().await?;
    tokio::time::sleep(Duration::from_millis(500)).await;
    click_center(page, &add_med_button).await?;
    eprintln!("[consult] clicked Add Med");

    let medicines_heading = text_matching_within(page, &Regex::new(r"(?i)^Medicines$").unwrap(), Duration::from_secs(20)).await?;
    expect(medicines_heading).with_timeout(Duration::from_secs(10)).to_be_visible().await?;
    eprintln!("[consult] add_medicine done");
    Ok(())
}

// Like the medicine field (and unlike Imaging Orders' free-text Test Name), this is an
// autocomplete -- a suggestion has to actually be clicked, e.g. "CBC" or "CBC+ESR" for a "CBC"
// search. Its own "Add Test" button shares that exact name with Imaging Orders' -- call this
// before add_imaging_test (so the Imaging Orders section isn't expanded yet) to keep that name
// unique on the page while this clicks it.
pub async fn add_investigation(page: &Page, search_term: &str) -> Result<()> {
    // The Medicines list just added above pushes every section below it (including this
    // toggle) further down the page, so its position can still be settling when we go to click
    // it -- same reflow race already guarded against for the Imaging Orders toggle below.
    let investigations_toggle = page.get_by_role(AriaRole::Button, Some(GetByRoleOptions::default().name("Investigations").exact(true))).first();
    let test_name_input = page
        .get_by_role(AriaRole::Textbox, Some(GetByRoleOptions::default().name("Search and select a lab test").exact(false)))
        .first();
    // Confirmed live: click_center can land fine (no error) while the reflow from the
    // Medicines list just above still shifts the toggle out from under it, so the panel never
    // actually opens -- the same class of race already guarded against for Medications' toggle
    // (see add_medicine above), just surfacing here as a silent no-op click instead of a thrown
    // error. Re-click on a short probe rather than trusting one click and waiting out the full
    // timeout for a panel that was never going to open.
    let mut opened = false;
    for attempt in 1..=3 {
        for scroll_attempt in 1..=3 {
            match tokio::time::timeout(Duration::from_secs(5), investigations_toggle.scroll_into_view_if_needed()).await {
                Ok(Ok(())) => break,
                _ if scroll_attempt < 3 => {
                    eprintln!("[consult] scroll_into_view_if_needed on Investigations toggle stalled, retrying (attempt {scroll_attempt})");
                    continue;
                }
                Ok(Err(e)) => return Err(e.into()),
                Err(_) => return Err(anyhow!("Timed out scrolling the Investigations toggle into view.")),
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
        click_center(page, &investigations_toggle).await?;
        eprintln!("[consult] clicked Investigations toggle (attempt {attempt})");

        if expect(test_name_input.clone()).with_timeout(Duration::from_secs(10)).to_be_visible().await.is_ok() {
            opened = true;
            break;
        }
        eprintln!("[consult] Investigations panel didn't open after click, retrying (attempt {attempt})");
    }
    if !opened {
        // Final, longer wait in case it's just slow rather than never-opened, surfaced as the
        // real error if it still never shows up.
        expect(test_name_input.clone()).with_timeout(Duration::from_secs(20)).to_be_visible().await?;
    }
    // Like the task search box in enter_appointment above, this field's accessible name/state
    // changes once a suggestion match appears -- confirmed live: that makes
    // type_into_verified's input_value() check unreliable here specifically, with the retry it
    // triggers on a spurious mismatch landing a re-click right into the suggestion dropdown's
    // own re-render, which is what was actually causing this call to hang/fail, not a dropped
    // keystroke. Retry against the signal that actually matters instead -- the suggestion
    // showing up -- the same fix already applied to that search box.
    let suggestion_pattern = Regex::new(&format!("(?i)^{}", regex::escape(search_term))).unwrap();
    let mut suggestion = None;
    for attempt in 1..=3 {
        if attempt > 1 {
            eprintln!("[consult] lab test suggestion not found yet, retyping (attempt {attempt})");
            click_center(page, &test_name_input).await?;
            page.keyboard().press("Control+A", None).await?;
            page.keyboard().press("Backspace", None).await?;
        }
        type_into(page, &test_name_input, search_term).await?;
        eprintln!("[consult] typed lab test search term");
        suggestion = first_matching_with_timeout(page.get_by_role(AriaRole::Button, None), &suggestion_pattern, Duration::from_secs(10)).await.ok();
        if suggestion.is_some() {
            break;
        }
    }
    let suggestion = suggestion.ok_or_else(|| anyhow!("Could not find a lab test suggestion matching \"{search_term}\" after retrying."))?;
    eprintln!("[consult] found lab test suggestion");
    click_center(page, &suggestion).await?;
    eprintln!("[consult] clicked lab test suggestion");

    let add_test_button = page.get_by_role(AriaRole::Button, Some(GetByRoleOptions::default().name("Add Test").exact(true))).first();
    add_test_button.scroll_into_view_if_needed().await?;
    tokio::time::sleep(Duration::from_millis(500)).await;
    click_center(page, &add_test_button).await?;
    eprintln!("[consult] clicked Add Test (investigation)");

    let lab_tests_heading = text_matching_within(page, &Regex::new(r"(?i)^Lab Tests$").unwrap(), Duration::from_secs(40)).await?;
    expect(lab_tests_heading).with_timeout(Duration::from_secs(10)).to_be_visible().await?;
    eprintln!("[consult] add_investigation done");
    Ok(())
}

// Unlike the medicine field, Test Name accepts free text directly -- no suggestion needs to
// be selected.
pub async fn add_imaging_test(page: &Page, test_name: &str, body_part: &str) -> Result<()> {
    // The Medicines list just added above pushes every section below it (including this
    // toggle) further down the page, so its position can still be settling when we go to
    // click it. Scroll it into view and let the layout settle first.
    let imaging_toggle = page.get_by_role(AriaRole::Button, Some(GetByRoleOptions::default().name("Imaging Orders").exact(true))).first();
    // Confirmed live in a sibling module: an unprotected Locator call here can hang the whole
    // test indefinitely rather than error, especially right after a page reflow (the Medicines
    // list just above just settled) -- give it its own timeout and retry rather than block
    // forever on a single stuck call.
    for attempt in 1..=3 {
        match tokio::time::timeout(Duration::from_secs(5), imaging_toggle.scroll_into_view_if_needed()).await {
            Ok(Ok(())) => break,
            _ if attempt < 3 => {
                eprintln!("[consult] scroll_into_view_if_needed on Imaging Orders toggle stalled, retrying (attempt {attempt})");
                continue;
            }
            Ok(Err(e)) => return Err(e.into()),
            Err(_) => return Err(anyhow!("Timed out scrolling the Imaging Orders toggle into view.")),
        }
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    click_center(page, &imaging_toggle).await?;
    eprintln!("[consult] clicked Imaging Orders toggle");

    // Confirmed live via screenshot: plain type_into can report success here while the field
    // stays visibly empty (same class of dropped-input race seen on the Instructions field
    // above) -- verify the text actually landed and retype if not, rather than trust the typed
    // call alone.
    let test_name_input = page
        .get_by_role(AriaRole::Textbox, Some(GetByRoleOptions::default().name("Search test name").exact(false)))
        .first();
    type_into_verified(page, &test_name_input, test_name).await?;
    eprintln!("[consult] typed test name");

    let body_part_input = page
        .get_by_role(AriaRole::Textbox, Some(GetByRoleOptions::default().name("e.g., chest, brain").exact(false)))
        .first();
    type_into_verified(page, &body_part_input, body_part).await?;
    eprintln!("[consult] typed body part");

    // Same class of layout-shift race as the toggle above: filling the fields can reflow the
    // section, so re-settle before clicking Add Test.
    let add_test_button = page.get_by_role(AriaRole::Button, Some(GetByRoleOptions::default().name("Add Test").exact(true))).first();
    add_test_button.scroll_into_view_if_needed().await?;
    tokio::time::sleep(Duration::from_millis(500)).await;
    click_center(page, &add_test_button).await?;
    eprintln!("[consult] clicked Add Test");

    tokio::time::sleep(Duration::from_secs(2)).await;
    if let Ok(bytes) = page.screenshot(None).await {
        let _ = std::fs::write("/tmp/consultation-debug-immediate.png", bytes);
        eprintln!("[consult] wrote immediate post-Add-Test screenshot");
    }

    // Confirmed live: the input fields clear immediately on a successful Add Test click (so the
    // click itself did register), but the "Test Orders" list confirming the add can take
    // noticeably longer than 20s to actually render on this remote test environment -- widen
    // the wait rather than treat a slow render as a failed add.
    let test_orders_heading = text_matching_within(page, &Regex::new(r"(?i)^Test Orders$").unwrap(), Duration::from_secs(40)).await?;
    expect(test_orders_heading).with_timeout(Duration::from_secs(10)).to_be_visible().await?;
    eprintln!("[consult] add_imaging_test done");
    Ok(())
}

// Saves (and forwards, per the "Forward this prescription to" section's Lab Service checkbox,
// checked by default) the prescription built up by add_medicine/add_investigation/
// add_imaging_test, then completes the consultation.
//
// "Save" was previously found positioned relative to a "Validate" button, since plain
// name-based lookup wasn't unique on this page -- confirmed live that this app version no
// longer has a Validate button at all, and "Save" (exact) is now uniquely findable directly.
// If Save is somehow clicked before any medicine/lab/procedure content exists, a "Write
// Prescription: Please type the prescription details..." dialog appears with just an "Ok"
// button; that dialog fully covers the page and blocks everything behind it (confirmed live:
// this was the real cause of an earlier, hard-to-explain flake where the Medications toggle
// seemed to intermittently become unclickable for no visible reason -- an unscoped "Save"
// click elsewhere in this flow was hitting this button instead of its intended target). Dismiss
// it defensively here too, though normal callers add real content first so it shouldn't trigger.
//
// Finish Appointment was found, in the TS version of this suite, to get stuck in a permanent
// loading spinner on this environment and never actually complete -- this waits a bounded
// amount of time for the appointment page to navigate away rather than hang indefinitely if
// that recurs, surfacing it as a clear error instead.
pub async fn save_and_finish_appointment(page: &Page) -> Result<()> {
    let save_button = page.get_by_role(AriaRole::Button, Some(GetByRoleOptions::default().name("Save").exact(true))).first();
    expect(save_button.clone()).with_timeout(Duration::from_secs(10)).to_be_visible().await?;
    click_center(page, &save_button).await?;
    eprintln!("[consult] clicked Save (forward prescription)");

    let write_prescription_ok = page.get_by_role(AriaRole::Button, Some(GetByRoleOptions::default().name("Ok").exact(true))).first();
    if expect(write_prescription_ok.clone()).with_timeout(Duration::from_secs(5)).to_be_visible().await.is_ok() {
        eprintln!("[consult] dismissing unexpected \"Write Prescription\" dialog after Save");
        click_center(page, &write_prescription_ok).await?;
    }

    let finish_button = page
        .get_by_role(AriaRole::Button, Some(GetByRoleOptions::default().name("Finish Appointment").exact(false)))
        .first();
    expect(finish_button.clone()).with_timeout(Duration::from_secs(15)).to_be_visible().await?;
    click_center(page, &finish_button).await?;
    eprintln!("[consult] clicked Finish Appointment");

    let start_url = page.url();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        if page.url() != start_url {
            eprintln!("[consult] navigated away after Finish Appointment: {}", page.url());
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(anyhow!(
                "Finish Appointment did not navigate away within 30s (last URL: {}) -- likely the known permanent-loading-spinner issue on this environment.",
                page.url()
            ));
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}
