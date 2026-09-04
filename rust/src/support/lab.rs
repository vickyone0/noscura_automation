use crate::support::flutter::{
    button_right_of, click_center, text_below, text_matching, type_into, type_into_verified, wait_for_url_matching,
};
use anyhow::{anyhow, Result};
use playwright_rs::expect;
use playwright_rs::protocol::locator::{AriaRole, GetByRoleOptions};
use playwright_rs::protocol::page::Page;
use regex::Regex;
use std::time::Duration;

// Searching a patient shows a single result card with an unlabeled arrow icon (no "Proceed"
// text, unlike the Outpatient booking flow) -- located relative to the patient's name text
// rather than by role+name.
async fn open_lab_booking_for_patient(page: &Page, patient_name: &str) -> Result<()> {
    let lab_tab = page.get_by_role(AriaRole::Button, Some(GetByRoleOptions::default().name("Laboratory").exact(true))).first();
    expect(lab_tab.clone()).with_timeout(Duration::from_secs(15)).to_be_visible().await?;
    click_center(page, &lab_tab).await?;

    // This field's accessible name becomes whatever was typed once it has a value (its
    // aria-label isn't a fixed placeholder), so it's targeted positionally rather than by
    // placeholder text.
    let search_box = page.get_by_role(AriaRole::Textbox, None).first();
    expect(search_box.clone()).with_timeout(Duration::from_secs(15)).to_be_visible().await?;
    type_into_verified(page, &search_box, patient_name).await?;

    // The patient's name can already be showing further down in the (unfiltered, still
    // loading) task list before the debounced search even returns -- text_below needs the
    // search-result card to already be there to succeed (it must sit just below the search
    // box, not wherever DOM order happens to put a same-named task-list row), so retry it
    // directly until a genuinely-nearby match shows up.
    let name_pattern = Regex::new(&format!("(?i)^{}$", regex::escape(patient_name))).unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    let result_name = loop {
        match text_below(page, &search_box, &name_pattern, 150.0).await {
            Ok(found) => break found,
            Err(e) => {
                if tokio::time::Instant::now() >= deadline {
                    return Err(e);
                }
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
        }
    };
    let result_box = result_name
        .bounding_box()
        .await?
        .ok_or_else(|| anyhow!("Could not read a bounding box for the search result."))?;
    let result_action = button_right_of(page, &result_box).await?;

    // This click reliably navigates in a plain script driving the same page, but not
    // consistently under the Playwright test runner against this live site -- retry it
    // rather than the whole test.
    for attempt in 1..=3 {
        click_center(page, &result_action).await?;

        // If the patient already has a pending lab task, a dialog asks whether to view it
        // or book another one anyway -- proceed with a new booking rather than getting
        // stuck on it.
        let confirm_button = page.get_by_role(AriaRole::Button, Some(GetByRoleOptions::default().name("Confirm").exact(true))).first();
        let dialog_shown = expect(confirm_button.clone())
            .with_timeout(Duration::from_secs(5))
            .to_be_visible()
            .await
            .is_ok();
        if dialog_shown {
            click_center(page, &confirm_button).await?;
        }

        let navigated = wait_for_url_matching(page, &Regex::new(r"serviceDetailsLab").unwrap(), Duration::from_secs(5))
            .await
            .is_ok();
        if navigated {
            return Ok(());
        }
        if attempt == 3 {
            return Err(anyhow!("Did not navigate to the lab booking form after {attempt} attempts."));
        }
    }
    Ok(())
}

// The Test Name field is an autocomplete, same shape as the medicine field in the
// consultation flow: typing alone doesn't add anything, a suggestion has to be selected.
// Selecting one auto-fills Description and Amount from the catalog entry.
async fn add_lab_test(page: &Page, test_name: &str) -> Result<()> {
    let test_name_input = page
        .get_by_role(AriaRole::Textbox, Some(GetByRoleOptions::default().name("e.g., : CBC").exact(false)))
        .first();
    expect(test_name_input.clone()).with_timeout(Duration::from_secs(10)).to_be_visible().await?;
    type_into(page, &test_name_input, test_name).await?;

    let suggestion = page
        .get_by_role(AriaRole::Button, Some(GetByRoleOptions::default().name(test_name).exact(true)))
        .first();
    expect(suggestion.clone()).with_timeout(Duration::from_secs(10)).to_be_visible().await?;
    click_center(page, &suggestion).await?;

    // Selecting a suggestion triggers Description/Amount to populate asynchronously, and
    // hitting that mid-churn can stall or land on the wrong button if the row's position
    // drifted since typing began. Let the fields finish settling first, then anchor off the
    // "Amount*" label, which keeps both a fixed name and a fixed position (unlike the Test
    // Name/Amount fields themselves, which switch from textbox to button-like display once a
    // suggestion is selected).
    tokio::time::sleep(Duration::from_secs(1)).await;
    let amount_label = text_matching(page, &Regex::new(r"(?i)^Amount\*$").unwrap()).await?;
    expect(amount_label.clone()).with_timeout(Duration::from_secs(10)).to_be_visible().await?;
    let amount_label_box = amount_label
        .bounding_box()
        .await?
        .ok_or_else(|| anyhow!("Could not read a bounding box for the Amount label."))?;

    // The "+" action button that commits this row to the billing table below is unlabeled --
    // it sits to the right of the Amount field/label, on the same row.
    let add_button = button_right_of(page, &amount_label_box).await?;
    click_center(page, &add_button).await?;

    // Confirm the row actually landed in the billing table before moving on, rather than
    // discovering a missed click much later at Submit time. Flutter merges the whole payment
    // summary into one text node, so match the amount as a substring rather than anchoring
    // to the start of the text.
    let total_shown = text_matching(page, &Regex::new(r"Rs\.\s*250").unwrap()).await?;
    expect(total_shown).with_timeout(Duration::from_secs(10)).to_be_visible().await?;
    Ok(())
}

pub async fn book_lab_test(page: &Page, patient_name: &str, test_name: &str, doctor_name: &str) -> Result<()> {
    open_lab_booking_for_patient(page, patient_name).await?;
    add_lab_test(page, test_name).await?;

    // Matches the TS reference (plain typeInto, no verification): confirmed live that
    // type_into_verified's *retry* can itself fail here -- the field can become briefly
    // unstable right after typing (from the billing table still settling), so by the time a
    // retry re-clicks to recover from an input_value() mismatch, the field has moved/been
    // replaced again and even the retry's own visibility check times out. Trying to add
    // verification here trades a rare dropped-input risk for a more frequent, harder failure.
    let doctor_input = page.get_by_role(AriaRole::Textbox, Some(GetByRoleOptions::default().name("Eg. John Doe").exact(false))).first();
    type_into(page, &doctor_input, doctor_name).await?;

    let submit_button = page.get_by_role(AriaRole::Button, Some(GetByRoleOptions::default().name("Submit").exact(true))).first();
    click_center(page, &submit_button).await?;

    // Submitting redirects back to the Laboratory dashboard's task list.
    wait_for_url_matching(page, &Regex::new(r"newHomeAdmin").unwrap(), Duration::from_secs(15)).await?;
    Ok(())
}
