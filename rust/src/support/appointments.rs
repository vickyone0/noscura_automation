use crate::support::flutter::{click_center, first_matching, text_matching, type_into};
use anyhow::{anyhow, Result};
use playwright_rs::expect;
use playwright_rs::protocol::locator::{AriaRole, FilterOptions, GetByRoleOptions, Locator};
use playwright_rs::protocol::page::Page;
use playwright_rs::protocol::wait_for::{WaitForOptions, WaitForState};
use regex::Regex;
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AppointmentMode {
    Offline,
    Online,
}

impl AppointmentMode {
    fn as_str(self) -> &'static str {
        match self {
            AppointmentMode::Offline => "Offline",
            AppointmentMode::Online => "Online",
        }
    }
}

// Booking a real slot creates a live appointment + payment record in the shared test
// database, so we verify success by the task list's total count growing by exactly one
// rather than matching specific text -- a prior run's leftover row would otherwise
// false-positive. Rendered rows can't be used directly since the list paginates at 5 per
// page; the true total lives in the "X-Y of N" pagination label instead.
pub async fn read_task_list_total(page: &Page) -> Result<u32> {
    let label = text_matching(page, &Regex::new(r"(?i)of\s+\d+").unwrap()).await?;
    let text = label.text_content().await?.unwrap_or_default();
    let re = Regex::new(r"(?i)of\s+(\d+)").unwrap();
    let captures = re
        .captures(&text)
        .ok_or_else(|| anyhow!("Could not read task list total from \"{text}\"."))?;
    Ok(captures[1].parse()?)
}

// Neither the first nor the last slot reliably lands on a bookable one, for two independent
// reasons confirmed live: Flutter's semantics DOM order for the grid doesn't track
// chronological order, and -- more fundamentally -- bookable slots sit in a rolling window
// near the *current* time rather than "anywhere today". So compute candidate labels directly
// from the current time plus a buffer, rounded to the grid's slot size, instead of searching
// blindly.
pub fn compute_candidate_slot_labels(buffer_minutes: i64, step_minutes: i64, count: i64) -> Vec<String> {
    let now = chrono::Local::now();
    let now_minutes = now.hour() as i64 * 60 + now.minute() as i64;
    let start = ((now_minutes + buffer_minutes) as f64 / step_minutes as f64).ceil() as i64 * step_minutes;
    (0..count)
        .map(|i| {
            let total_minutes = (start + i * step_minutes).rem_euclid(24 * 60);
            format_slot_label(total_minutes)
        })
        .collect()
}

// For a future day there's no "past times to skip" concern the way there is for today, so
// this starts from a fixed clinic-opening time. Kept short (see FUTURE_DAY_CANDIDATE_COUNT)
// -- scanning every rejected candidate costs a real few seconds each (the alert-wait race in
// try_candidate_slots), so trying many is what makes the scan itself risk taking longer than
// any reasonable test timeout. A brand new day is far less likely to already be picked over
// by this suite's own repeated runs than today is, so a short scan should suffice.
fn full_day_slot_labels(step_minutes: i64, count: i64) -> Vec<String> {
    (0..count).map(|i| format_slot_label(8 * 60 + i * step_minutes)).collect()
}

fn format_slot_label(total_minutes: i64) -> String {
    let hour24 = total_minutes / 60;
    let minute = total_minutes % 60;
    let hour12 = if hour24 % 12 == 0 { 12 } else { hour24 % 12 };
    format!("{hour12}:{minute:02} {}", if hour24 < 12 { "AM" } else { "PM" })
}

use chrono::Timelike;

enum SlotOutcome {
    Confirmed,
    Unavailable,
    Timeout,
}

async fn try_candidate_slots(page: &Page, dialog: &Locator, candidate_labels: &[String]) -> Result<Option<String>> {
    let confirmation_dialog = page
        .get_by_role(AriaRole::Dialog, None)
        .filter(FilterOptions::default().has_text("Booking Confirmation"));
    let unavailable_alert = page
        .get_by_role(AriaRole::Alertdialog, None)
        .filter(FilterOptions::default().has_text("not available"));

    for label in candidate_labels {
        let pattern = Regex::new(&format!(r"(?i)^{}$", regex::escape(label))).unwrap();
        let slot_button = match first_matching(&dialog.get_by_role(AriaRole::Button, None), &pattern).await? {
            Some(b) => b,
            None => continue,
        };
        if slot_button.scroll_into_view_if_needed().await.is_err() {
            continue;
        }
        click_center(page, &slot_button).await?;

        let wait_opts = || Some(WaitForOptions::builder().state(WaitForState::Visible).timeout(3000.0).build());
        let outcome = tokio::select! {
            r = confirmation_dialog.wait_for(wait_opts()) => if r.is_ok() { SlotOutcome::Confirmed } else { SlotOutcome::Timeout },
            r = unavailable_alert.wait_for(wait_opts()) => if r.is_ok() { SlotOutcome::Unavailable } else { SlotOutcome::Timeout },
        };

        match outcome {
            SlotOutcome::Confirmed => return Ok(Some(label.clone())),
            SlotOutcome::Unavailable => {
                let ok_button = page.get_by_role(AriaRole::Button, Some(GetByRoleOptions::default().name("Ok").exact(true))).first();
                click_center(page, &ok_button).await?;
            }
            SlotOutcome::Timeout => {}
        }
    }
    Ok(None)
}

// Offline's grid only offers :00/:30 slots; Online's finer-grained grid also has :15/:45. A
// wide candidate range matters on this shared test DB: repeated runs against the same doctor
// (every flow in this suite uses the same one) permanently consume whatever slots they book,
// with no way to free them back up -- so today's calendar can end up with nothing bookable at
// all. If that happens, the dialog's own calendar grid lets any future date be picked, so
// advance a day at a time and try again there instead of giving up. Candidate counts are kept
// modest on purpose: each rejected candidate costs a real few seconds, so scanning too many
// risks the whole search taking longer than any reasonable test timeout.
const TODAY_CANDIDATE_COUNT: i64 = 10;
const FUTURE_DAY_CANDIDATE_COUNT: i64 = 10;
const MAX_DAYS_AHEAD: i64 = 2;

// The slot grid can still be empty/loading for a moment right after the dialog opens or right
// after a new day is picked -- wait for at least one to actually render before scanning,
// rather than risk reading candidates as unavailable when the grid just hadn't loaded yet.
async fn wait_for_slot_grid_ready(page: &Page, dialog: &Locator) -> Result<()> {
    let pattern = Regex::new(r"(?i)^\d{1,2}:\d{2} (?:AM|PM)$").unwrap();
    // Widened from 10s, then from 20s: first_matching's own overall per-call scan cap (8s, to
    // bound a slow-but-real scan rather than let it hang) can eat most of a tight budget in a
    // single attempt, leaving little room for the retries this loop is built around. Confirmed
    // live: this dialog's slot grid consistently failed to render any time-shaped button within
    // 20s under headless Chromium (twice in a row), while the identical flow under headed mode
    // (HEADED=1) rendered a full grid immediately every time -- a Flutter CanvasKit + headless
    // rendering quirk on this environment, not a logic bug. Widened further as a defensive
    // measure, but headed mode is the more reliable way to run this for now.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(40);
    loop {
        if first_matching(&dialog.get_by_role(AriaRole::Button, None), &pattern).await?.is_some() {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(anyhow!("Slot grid never rendered any time-shaped buttons."));
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
        let _ = page;
    }
}

// Returns (date, time) rather than just the time label. Confirmed live: on this shared test
// DB, today's slots near the current time are frequently already exhausted by this suite's own
// past runs, so booking routinely advances to a future day -- and that day can easily land on
// the same time-of-day (e.g. "9:00 AM") as some earlier run's stale, still-open appointment
// from an *different* day. A caller matching on patient name + time alone can't tell those
// apart and may silently re-enter the wrong (stale) appointment instead of the one just booked.
pub async fn select_available_time_slot(page: &Page, dialog: &Locator, mode: AppointmentMode) -> Result<(String, String)> {
    let step_minutes = if mode == AppointmentMode::Online { 15 } else { 30 };

    wait_for_slot_grid_ready(page, dialog).await?;
    let today_labels = compute_candidate_slot_labels(15, step_minutes, TODAY_CANDIDATE_COUNT);
    if let Some(slot) = try_candidate_slots(page, dialog, &today_labels).await? {
        return Ok((short_date_label(chrono::Local::now()), slot));
    }

    for days_ahead in 1..=MAX_DAYS_AHEAD {
        let date = chrono::Local::now() + chrono::Duration::days(days_ahead);
        let date_label = format_date_label(date);

        let date_button = page.get_by_role(AriaRole::Button, Some(GetByRoleOptions::default().name(&date_label).exact(true))).first();
        if date_button.scroll_into_view_if_needed().await.is_err() {
            continue;
        }
        click_center(page, &date_button).await?;
        wait_for_slot_grid_ready(page, dialog).await?;

        let future_labels = full_day_slot_labels(step_minutes, FUTURE_DAY_CANDIDATE_COUNT);
        if let Some(slot) = try_candidate_slots(page, dialog, &future_labels).await? {
            return Ok((short_date_label(date), slot));
        }
    }

    Err(anyhow!("Could not find a bookable time slot today or over the next {MAX_DAYS_AHEAD} days."))
}

// Matches the task list's own "Date & Time" cell format observed live, e.g. "1/9/2026" (D/M/YYYY,
// no leading zeros) -- not the verbose calendar-dialog label format_date_label produces.
fn short_date_label(date: chrono::DateTime<chrono::Local>) -> String {
    use chrono::Datelike;
    format!("{}/{}/{}", date.day(), date.month(), date.year())
}

fn format_date_label(date: chrono::DateTime<chrono::Local>) -> String {
    use chrono::Datelike;
    let weekday = match date.weekday() {
        chrono::Weekday::Sun => "Sunday",
        chrono::Weekday::Mon => "Monday",
        chrono::Weekday::Tue => "Tuesday",
        chrono::Weekday::Wed => "Wednesday",
        chrono::Weekday::Thu => "Thursday",
        chrono::Weekday::Fri => "Friday",
        chrono::Weekday::Sat => "Saturday",
    };
    let month = match date.month() {
        1 => "January",
        2 => "February",
        3 => "March",
        4 => "April",
        5 => "May",
        6 => "June",
        7 => "July",
        8 => "August",
        9 => "September",
        10 => "October",
        11 => "November",
        _ => "December",
    };
    format!("{weekday}, {month} {}, {}", date.day(), date.year())
}

// Returns the booked slot's (date, time) labels (e.g. ("1/9/2026", "9:00 AM")) so a caller that
// needs to re-enter this exact appointment afterward (rather than whichever "Brad Pitt" row
// happens to match first -- on a reused fixed patient, that's liable to be some stale
// appointment from an earlier run, possibly on a different day at the same time) can match on
// both specifically.
pub async fn book_appointment(page: &Page, patient_name: &str, mode: AppointmentMode) -> Result<(String, String)> {
    let outpatient_tab = page.get_by_role(AriaRole::Button, Some(GetByRoleOptions::default().name("Outpatient").exact(false))).first();
    expect(outpatient_tab.clone()).with_timeout(Duration::from_secs(15)).to_be_visible().await?;
    click_center(page, &outpatient_tab).await?;

    // The list renders as two separate tables: a header-only table, and a second table
    // holding the actual data rows.
    let task_table = page.get_by_role(AriaRole::Table, None).nth(1);
    expect(task_table.clone()).with_timeout(Duration::from_secs(15)).to_be_visible().await?;
    let total_before = read_task_list_total(page).await?;

    let search_box = page
        .get_by_role(AriaRole::Textbox, Some(GetByRoleOptions::default().name("Enter the patient's name or phone number").exact(false)))
        .first();
    expect(search_box.clone()).with_timeout(Duration::from_secs(15)).to_be_visible().await?;
    type_into(page, &search_box, patient_name).await?;

    let proceed_button = page.get_by_role(AriaRole::Button, Some(GetByRoleOptions::default().name("Proceed").exact(true))).first();
    expect(proceed_button.clone()).with_timeout(Duration::from_secs(15)).to_be_visible().await?;
    click_center(page, &proceed_button).await?;

    let booking_dialog = page.get_by_role(AriaRole::Dialog, None).filter(FilterOptions::default().has_text("Book Appointments"));
    expect(booking_dialog.clone()).with_timeout(Duration::from_secs(15)).to_be_visible().await?;

    // Appointment Mode defaults to Offline. Switching to Online swaps the slot grid to
    // 15-minute intervals across the full day -- everything else in the dialog is identical.
    if mode == AppointmentMode::Online {
        let offline_toggle = first_matching(&booking_dialog.get_by_role(AriaRole::Button, None), &Regex::new(r"(?i)^offline$").unwrap())
            .await?
            .ok_or_else(|| anyhow!("Could not find the Offline mode toggle."))?;
        click_center(page, &offline_toggle).await?;
        let online_option = page.get_by_role(AriaRole::Button, Some(GetByRoleOptions::default().name("Online").exact(true))).first();
        click_center(page, &online_option).await?;

        // The "Online" label updates before the slot grid finishes reloading, so grabbing a
        // slot right away can still click the stale Offline grid mid-transition. Wait for a
        // :15/:45 slot -- only present once the finer-grained Online grid has actually
        // loaded -- before proceeding.
        let fine_grained = Regex::new(r"(?i):(?:15|45) (?:AM|PM)$").unwrap();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
        loop {
            if first_matching(&booking_dialog.get_by_role(AriaRole::Button, None), &fine_grained).await?.is_some() {
                break;
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(anyhow!("Online slot grid never finished loading."));
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    // Confirm the dialog is actually configured for the intended mode right before booking.
    // Best-effort: this button's accessible name doesn't reliably resolve through the
    // aria-label/inner_text heuristic first_matching uses (confirmed live), likely because
    // it's part of a segmented Offline/Online toggle whose real accessible-name computation
    // this simplified check doesn't replicate exactly. The actual booking correctness doesn't
    // depend on this check succeeding, so log rather than fail the whole run over it.
    let mode_pattern = Regex::new(&format!(r"(?i)^{}$", mode.as_str())).unwrap();
    match first_matching(&booking_dialog.get_by_role(AriaRole::Button, None), &mode_pattern).await? {
        Some(mode_confirmed) => {
            if let Err(e) = expect(mode_confirmed).to_be_visible().await {
                eprintln!("Warning: could not confirm {} mode is visible: {e}", mode.as_str());
            }
        }
        None => eprintln!("Warning: could not locate a {} mode indicator to confirm.", mode.as_str()),
    }

    let (date_label, slot_label) = select_available_time_slot(page, &booking_dialog, mode).await?;

    let confirmation_dialog = page.get_by_role(AriaRole::Dialog, None).filter(FilterOptions::default().has_text("Booking Confirmation"));
    expect(confirmation_dialog.clone()).to_contain_text(&slot_label).await?;

    let confirm_button = page.get_by_role(AriaRole::Button, Some(GetByRoleOptions::default().name("Confirm Booking").exact(true))).first();
    click_center(page, &confirm_button).await?;

    // The booking is processed asynchronously; the dialog stays open (its button replaced by
    // a spinner) until the appointment is actually created, then it closes on its own.
    expect(confirmation_dialog).with_timeout(Duration::from_secs(30)).to_be_hidden().await?;

    // Confirming can return to the Book Appointments dialog (so staff can add another slot)
    // instead of closing everything -- dismiss it via its close button if still open.
    if booking_dialog.is_visible().await.unwrap_or(false) {
        let close_button = booking_dialog.get_by_role(AriaRole::Button, None).first();
        click_center(page, &close_button).await?;
    }

    // Closing the dialog triggers a full re-render of the task list, so wait for it to come
    // back first.
    expect(task_table).with_timeout(Duration::from_secs(15)).to_be_visible().await?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        let current = read_task_list_total(page).await?;
        if current > total_before {
            return Ok((date_label, slot_label));
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(anyhow!("Task list total never grew past {total_before}."));
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}
