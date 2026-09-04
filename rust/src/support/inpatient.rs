use crate::support::flutter::{
    button_matching, button_right_of, click_center, dropdown_near_label, select_dropdown_option, select_first_dropdown_option,
    text_matching, text_matching_within, type_into, type_into_verified, wait_for_url_matching,
};
use crate::support::new_patient::EXISTING_PATIENT_NAME;
use anyhow::{anyhow, Result};
use playwright_rs::expect;
use playwright_rs::protocol::locator::{AriaRole, FilterOptions, GetByRoleOptions, Locator};
use playwright_rs::protocol::page::Page;
use regex::Regex;
use std::time::Duration;

// Admitting a patient who already has an active admission sends the search result to a "view
// admission" page instead of the admit form -- and admissions here are one-way, with no
// discharge flow this suite exercises to free a patient back up. This reuses a fixed patient
// (EXISTING_PATIENT_NAME) rather than creating a fresh one, so this test only succeeds on its
// first run against a given environment; subsequent runs will hit that already-admitted state.

// The search result is one wide card; the only element inside it with its own accessible name
// is a small unlabeled chevron icon, but clicking that precise ~24x24 icon doesn't reliably
// register a click. The card's own outer area is a separate, much larger button covering the
// whole row, and clicking that navigates reliably -- so target the widest button that renders
// just below the search box rather than the icon inside it.
async fn open_admission_form_for_patient(page: &Page, patient_name: &str) -> Result<()> {
    let inpatient_tab = page.get_by_role(AriaRole::Button, Some(GetByRoleOptions::default().name("Inpatient").exact(true))).first();
    expect(inpatient_tab.clone()).with_timeout(Duration::from_secs(15)).to_be_visible().await?;
    click_center(page, &inpatient_tab).await?;

    let search_box = page.get_by_role(AriaRole::Textbox, None).first();
    expect(search_box.clone()).with_timeout(Duration::from_secs(15)).to_be_visible().await?;
    type_into_verified(page, &search_box, patient_name).await?;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    let result_row: Locator = loop {
        let attempt: Result<Locator> = async {
            let search_box_box = search_box
                .bounding_box()
                .await?
                .ok_or_else(|| anyhow!("Search box is not ready yet."))?;

            let buttons = page.get_by_role(AriaRole::Button, None);
            let count = match tokio::time::timeout(Duration::from_secs(3), buttons.count()).await {
                Ok(r) => r?,
                Err(_) => return Err(anyhow!("Button count timed out.")),
            };
            let mut found: Option<i32> = None;
            for i in 0..count as i32 {
                // A still-rendering page can shift the DOM mid-scan, leaving a given index
                // pointing at a since-detached node -- give each read its own short timeout
                // and skip it on failure rather than risk one stale node wedging the scan
                // (confirmed live in a sibling module: an unprotected version of this exact
                // kind of loop hung indefinitely).
                let box_ = match tokio::time::timeout(Duration::from_millis(800), buttons.nth(i).bounding_box()).await {
                    Ok(Ok(Some(b))) => b,
                    _ => continue,
                };
                let vertical_gap = box_.y - (search_box_box.y + search_box_box.height);
                if vertical_gap > 0.0 && vertical_gap < 150.0 && box_.width > 500.0 {
                    found = Some(i);
                }
            }
            match found {
                Some(i) => Ok(buttons.nth(i)),
                None => Err(anyhow!("Search result row has not rendered yet.")),
            }
        }
        .await;

        match attempt {
            Ok(row) => break row,
            Err(e) => {
                if tokio::time::Instant::now() >= deadline {
                    return Err(e);
                }
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
        }
    };

    click_center(page, &result_row).await?;
    wait_for_url_matching(page, &Regex::new(r"admitPatient").unwrap(), Duration::from_secs(15)).await?;
    Ok(())
}

// Department, Doctor, and Emergency Contact Full Name are required fields on this form.
async fn fill_required_admission_details(page: &Page, emergency_contact_name: &str) -> Result<()> {
    let dept_dropdown = dropdown_near_label(page, &Regex::new(r"(?i)^Department\*$").unwrap()).await?;
    select_dropdown_option(page, &dept_dropdown, &Regex::new(r"(?i)^General Medicine$").unwrap()).await?;

    // The Doctor list is scoped to whichever Department was just picked, so it must be
    // re-located fresh rather than assumed to be at a fixed position.
    let doctor_dropdown = dropdown_near_label(page, &Regex::new(r"(?i)^Doctor\*$").unwrap()).await?;
    select_dropdown_option(page, &doctor_dropdown, &Regex::new(r"(?i)kishan").unwrap()).await?;

    let full_name_input = page
        .get_by_role(AriaRole::Textbox, Some(GetByRoleOptions::default().name("e.g., : John Doe").exact(false)))
        .first();
    type_into(page, &full_name_input, emergency_contact_name).await?;
    Ok(())
}

// Every Room Type this environment offers, confirmed live via its dropdown's option list.
// Male Ward is tried first since it matches the default patient gender created here; the
// rest are fallbacks purely to find an open bed, not a clinically appropriate match.
const ROOM_TYPES_TO_TRY: &[&str] = &["Male Ward", "AC- Rooms", "ICU", "Non AC Room", "Female Ward"];

// Unlike dropdown_near_label (which locates a control by its "Please select..." placeholder
// text), this finds a dropdown control by its label regardless of current value -- needed for
// Room Type and Floor/Block specifically, since select_room_type_and_bed re-opens both after
// a first pick already replaced their placeholders with a chosen value.
async fn control_near_label(page: &Page, label_pattern: &Regex) -> Result<Locator> {
    let label = text_matching(page, label_pattern).await?;
    label.scroll_into_view_if_needed().await?;
    let label_box = label
        .bounding_box()
        .await?
        .ok_or_else(|| anyhow!("Could not find a label matching {label_pattern}."))?;

    let buttons = page.get_by_role(AriaRole::Button, None);
    let count = match tokio::time::timeout(Duration::from_secs(3), buttons.count()).await {
        Ok(r) => r?,
        Err(_) => return Err(anyhow!("Button count timed out.")),
    };
    let mut best: Option<(i32, f64)> = None;
    for i in 0..count as i32 {
        let box_ = match tokio::time::timeout(Duration::from_millis(800), buttons.nth(i).bounding_box()).await {
            Ok(Ok(Some(b))) => b,
            _ => continue,
        };
        let vertical_gap = box_.y - label_box.y;
        if !(0.0..=60.0).contains(&vertical_gap) {
            continue;
        }
        let horizontal_gap = (label_box.x - (box_.x + box_.width)).max(box_.x - (label_box.x + label_box.width)).max(0.0);
        if horizontal_gap > 300.0 {
            continue;
        }
        let distance = vertical_gap + horizontal_gap;
        if best.map_or(true, |(_, d)| distance < d) {
            best = Some((i, distance));
        }
    }
    let (index, _) = best.ok_or_else(|| anyhow!("Could not find a control near a label matching {label_pattern}."))?;
    Ok(buttons.nth(index))
}

// Room Type -> Floor/Block -> a "Bed Manager" dialog listing individual beds (e.g.
// "WARD-M - 205 - BED1"). Repeated runs (this suite's own past ones included) permanently
// occupy beds with no discharge flow to free them, so a room type that had space yesterday
// can show "All beds are full." today -- try each room type in turn rather than assuming any
// one of them still has room.
async fn select_room_type_and_bed(page: &Page) -> Result<()> {
    for room_type in ROOM_TYPES_TO_TRY {
        eprintln!("[inpatient] trying room type: {room_type}");
        let room_type_control = control_near_label(page, &Regex::new(r"(?i)^Room Type\*$").unwrap()).await?;
        select_dropdown_option(page, &room_type_control, &Regex::new(&format!("(?i)^{}$", regex::escape(room_type))).unwrap()).await?;
        eprintln!("[inpatient] selected room type");

        // Only one Floor/Block value is configured per room type in this environment; any
        // one option is fine, so pick whichever renders rather than hardcoding it.
        let floor_block_control = control_near_label(page, &Regex::new(r"(?i)^Floor/Block\*?$").unwrap()).await?;
        select_first_dropdown_option(page, &floor_block_control).await?;
        eprintln!("[inpatient] selected floor/block");

        let select_bed = text_matching(page, &Regex::new(r"(?i)^Select Bed$").unwrap()).await?;
        expect(select_bed.clone()).with_timeout(Duration::from_secs(10)).to_be_visible().await?;
        click_center(page, &select_bed).await?;
        eprintln!("[inpatient] clicked Select Bed");

        let bed_manager_dialog = page.get_by_role(AriaRole::Dialog, None).filter(FilterOptions::default().has_text("Bed Manager"));
        expect(bed_manager_dialog.clone()).with_timeout(Duration::from_secs(10)).to_be_visible().await?;
        eprintln!("[inpatient] Bed Manager dialog visible");

        let bed_pattern = Regex::new(r"(?i)WARD.*BED\d+").unwrap();
        let bed_option = crate::support::flutter::first_matching(&bed_manager_dialog.get_by_role(AriaRole::Button, None), &bed_pattern).await?;

        if let Some(bed_option) = bed_option {
            let bed_available = expect(bed_option.clone()).with_timeout(Duration::from_secs(5)).to_be_visible().await.is_ok();
            if bed_available {
                click_center(page, &bed_option).await?;
                // Picking a bed reflows the rest of the form; reading positions immediately
                // after this click intermittently catches the pre-reflow layout.
                tokio::time::sleep(Duration::from_millis(800)).await;
                return Ok(());
            }
        }

        // "All beds are full." -- close the dialog via its only (unlabeled) button and try
        // the next room type.
        let mut dialog_closed = false;
        for _ in 1..=3 {
            let close_button = bed_manager_dialog.get_by_role(AriaRole::Button, None).first();
            click_center(page, &close_button).await?;
            dialog_closed = expect(bed_manager_dialog.clone()).with_timeout(Duration::from_secs(5)).to_be_hidden().await.is_ok();
            if dialog_closed {
                break;
            }
        }
        if !dialog_closed {
            return Err(anyhow!("Could not close the full \"Bed Manager\" dialog."));
        }
    }
    Err(anyhow!("No room type had an available bed among {}.", ROOM_TYPES_TO_TRY.join(", ")))
}

// Selecting Payment Mode after typing the Advance Amount can leave the amount field
// rendering empty again (a Flutter widget-rebuild side effect, not a real navigation or clear
// action) -- so Payment Mode is selected first, and Advance Amount is filled last, right
// before Submit, to land after any such rebuild rather than before it.
async fn fill_billing_details(page: &Page, advance_amount: &str) -> Result<()> {
    let payment_mode_dropdown = dropdown_near_label(page, &Regex::new(r"(?i)^Payment Mode$").unwrap()).await?;
    select_dropdown_option(page, &payment_mode_dropdown, &Regex::new(r"(?i)^cash$").unwrap()).await?;

    let advance_amount_input = page.get_by_role(AriaRole::Textbox, Some(GetByRoleOptions::default().name("00").exact(true))).first();
    type_into(page, &advance_amount_input, advance_amount).await?;
    Ok(())
}

pub async fn admit_patient(page: &Page, emergency_contact_name: &str) -> Result<String> {
    let patient_name = EXISTING_PATIENT_NAME.to_string();

    open_admission_form_for_patient(page, &patient_name).await?;
    fill_required_admission_details(page, emergency_contact_name).await?;
    select_room_type_and_bed(page).await?;
    fill_billing_details(page, "500").await?;

    let submit_button = page.get_by_role(AriaRole::Button, Some(GetByRoleOptions::default().name("Submit").exact(true))).first();
    click_center(page, &submit_button).await?;

    // Submitting redirects back to the Inpatient dashboard's patient list.
    wait_for_url_matching(page, &Regex::new(r"newHomeAdmin").unwrap(), Duration::from_secs(15)).await?;
    Ok(patient_name)
}

// After admission, the patient shows up as a row in the Inpatient dashboard's own Patient
// List table -- but `newHomeAdmin` defaults back to the Admin tab's general, cross-module
// Patients List on a fresh navigation (confirmed live: even right after an admission
// redirects here, the visible tab is Admin, not Inpatient). A row matching the patient's name
// exists on *both* lists, so a name-only search without first switching to the Inpatient tab
// can silently find the wrong one -- its Action button opens a different page entirely, not
// serviceDetailsIP. Switch to Inpatient and filter via its own Patient List search box
// (distinct from the search-driven admission-form flow's search box, used before an admission
// exists) to land on the right row.
pub async fn open_patient_from_dashboard_list(page: &Page, patient_name: &str) -> Result<()> {
    let inpatient_tab = page.get_by_role(AriaRole::Button, Some(GetByRoleOptions::default().name("Inpatient").exact(true))).first();
    click_center(page, &inpatient_tab).await?;

    let task_search_box = page
        .get_by_role(AriaRole::Textbox, Some(GetByRoleOptions::default().name("Search tasks by Patient name").exact(false)))
        .first();
    expect(task_search_box.clone()).with_timeout(Duration::from_secs(15)).to_be_visible().await?;
    type_into(page, &task_search_box, patient_name).await?;

    let row = page.get_by_role(AriaRole::Row, Some(GetByRoleOptions::default().name(patient_name).exact(false))).first();
    expect(row.clone()).with_timeout(Duration::from_secs(15)).to_be_visible().await?;

    // Landing here right off the back of a just-submitted admission (rather than a fresh,
    // separate navigation) can catch a transient overlay/notification still settling in from
    // that submission, which can swallow a single click with no visible error -- retry rather
    // than treat one missed click as fatal.
    let service_details_pattern = Regex::new(r"serviceDetailsIP").unwrap();
    for attempt in 1..=3 {
        let action_button = row.get_by_role(AriaRole::Button, None).first();
        click_center(page, &action_button).await?;
        if wait_for_url_matching(page, &service_details_pattern, Duration::from_secs(5)).await.is_ok() {
            return Ok(());
        }
        if attempt == 3 {
            return Err(anyhow!(
                "Clicking the patient row's action button never navigated to serviceDetailsIP. Last URL: {}",
                page.url()
            ));
        }
        eprintln!("[inpatient] row action click didn't navigate yet, retrying (attempt {attempt})");
    }
    Ok(())
}

// Room Charges starts unbilled (Subtotal Rs. 0) on a freshly admitted patient -- the "+"
// button next to it adds it to the bill. This assumes that starting state: on a patient that
// already has Room Charges billed, the same button has become a delete icon, so calling this
// again would remove them instead of adding them.
//
// admit_patient always pays a fixed Rs. 500 advance, and Room Charges is Rs. 500/day -- so for
// the same-day (Day 1) stay this pipeline always produces, adding Room Charges already fully
// covers the bill and it settles immediately with no more to fill in. Only when there's an
// actual balance left (confirmed live: a multi-day admission from outside this pipeline) does
// a "Collect Payment" form appear, needing Generated By and Print Receipt set before Submit.
// Print Receipt is set to "No" rather than "Yes" there: selecting Yes tries to upload a PDF
// receipt to Firebase Storage, which 403s in this environment (the bill still submits and the
// payment is still recorded either way, but Yes leaves a failed upload behind).
pub async fn submit_final_bill(page: &Page, generated_by: &str) -> Result<()> {
    let billing_tab = page.get_by_role(AriaRole::Button, Some(GetByRoleOptions::default().name("Billing").exact(true))).first();
    click_center(page, &billing_tab).await?;

    let final_bill_tab = page.get_by_role(AriaRole::Button, Some(GetByRoleOptions::default().name("Final Bill").exact(true))).first();
    expect(final_bill_tab.clone()).with_timeout(Duration::from_secs(15)).to_be_visible().await?;
    click_center(page, &final_bill_tab).await?;

    let room_charges_label = text_matching(page, &Regex::new(r"(?i)^Room\s*Charges$").unwrap()).await?;
    let room_charges_box = room_charges_label
        .bounding_box()
        .await?
        .ok_or_else(|| anyhow!("Could not read a bounding box for the Room Charges label."))?;
    let add_room_charges = button_right_of(page, &room_charges_box).await?;
    click_center(page, &add_room_charges).await?;

    let needs_payment = text_matching_within(page, &Regex::new(r"(?i)Collect Rs\.").unwrap(), Duration::from_secs(5)).await;
    if needs_payment.is_ok() {
        let generated_by_input = page
            .get_by_role(AriaRole::Textbox, Some(GetByRoleOptions::default().name("Eg. John Doe").exact(false)))
            .first();
        expect(generated_by_input.clone()).with_timeout(Duration::from_secs(10)).to_be_visible().await?;
        type_into_verified(page, &generated_by_input, generated_by).await?;

        let print_receipt_no = page.get_by_role(AriaRole::Group, Some(GetByRoleOptions::default().name("No").exact(true))).first();
        click_center(page, &print_receipt_no).await?;

        let submit_button = button_matching(page, &Regex::new(r"(?i)^Submit").unwrap()).await?;
        click_center(page, &submit_button).await?;
    }

    // Either path -- collecting a balance via Submit, or Room Charges already being fully
    // covered by the advance -- ends with the same settled summary.
    text_matching(page, &Regex::new(r"(?i)Net payable").unwrap()).await?;
    Ok(())
}

// Discharges the patient, bypassing the full clinical Discharge Summary form (Final
// Diagnosis, History, Investigations, etc. are all required there and this suite has no real
// clinical narrative to put in them). This is what frees the patient up for their next
// admission in a subsequent run -- admitting an already-admitted patient routes to a "view
// admission" page instead of the admit form.
pub async fn discharge_patient(page: &Page) -> Result<()> {
    let discharge_tab = page.get_by_role(AriaRole::Button, Some(GetByRoleOptions::default().name("Discharge").exact(true))).first();
    click_center(page, &discharge_tab).await?;

    let skip_summary = page
        .get_by_role(AriaRole::Button, Some(GetByRoleOptions::default().name("Skip Discharge Summary").exact(true)))
        .first();
    expect(skip_summary.clone()).with_timeout(Duration::from_secs(15)).to_be_visible().await?;

    // This click can silently fail to register (confirmed live: the Discharge tab was reached
    // fine but stayed on its untouched initial state -- "Skip Discharge Summary" still sitting
    // there, no warning dialog ever appeared, and the discharge never happened despite the test
    // reporting success) -- retry rather than assume one click landed.
    let discharge_warning = page.get_by_role(AriaRole::Alertdialog, None).first();
    let mut dialog_shown = false;
    for attempt in 1..=3 {
        click_center(page, &skip_summary).await?;
        dialog_shown = expect(discharge_warning.clone()).with_timeout(Duration::from_secs(5)).to_be_visible().await.is_ok();
        if dialog_shown {
            break;
        }
        eprintln!("[inpatient] Skip Discharge Summary click didn't open a dialog yet, retrying (attempt {attempt})");
    }
    if !dialog_shown {
        return Err(anyhow!("Clicking \"Skip Discharge Summary\" never opened its confirmation dialog."));
    }

    // Confirms whichever warning dialog(s) this triggers -- "Discharge Summary Not
    // Attached" always, and a second "Final bill is not generated" one right after (confirmed
    // live: this shows up regardless of the Final Bill's Print Receipt choice).
    for _ in 0..2 {
        let confirm = page.get_by_role(AriaRole::Button, Some(GetByRoleOptions::default().name("Confirm").exact(true))).first();
        let shown = expect(confirm.clone()).with_timeout(Duration::from_secs(5)).to_be_visible().await.is_ok();
        if !shown {
            break;
        }
        click_center(page, &confirm).await?;
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    // Confirming successfully navigates away from serviceDetailsIP back to the dashboard --
    // verify that actually happened rather than trust the confirm clicks landed.
    wait_for_url_matching(page, &Regex::new(r"newHomeAdmin").unwrap(), Duration::from_secs(15))
        .await
        .map_err(|e| anyhow!("Confirming discharge never navigated away from serviceDetailsIP: {e}"))?;

    Ok(())
}
