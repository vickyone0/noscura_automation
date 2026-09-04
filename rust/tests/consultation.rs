// To run: RUN_NOSCURA_LOGIN_SMOKE=1 LOGIN_EMAIL=<email> LOGIN_PASSWORD=<password> cargo test --test consultation -- --test-threads=1
use noscura_e2e::support::appointments::{book_appointment, AppointmentMode};
use noscura_e2e::support::config;
use noscura_e2e::support::consultation::{add_imaging_test, add_investigation, add_medicine, enter_appointment, save_and_finish_appointment};
use noscura_e2e::support::flutter::text_matching;
use noscura_e2e::support::new_patient::EXISTING_PATIENT_NAME;
use noscura_e2e::support::auth;
use playwright_rs::expect;
use playwright_rs::protocol::page::Page;
use playwright_rs::protocol::playwright::Playwright;
use regex::Regex;

fn skip_reason() -> Option<&'static str> {
    if !config::run_login_smoke() {
        return Some("Set RUN_NOSCURA_LOGIN_SMOKE=1 to run the external Noscura admin login smoke test.");
    }
    if config::login_email().is_none() {
        return Some("Set LOGIN_EMAIL before running this test.");
    }
    if config::login_password().is_none() {
        return Some("Set LOGIN_PASSWORD before running this test.");
    }
    None
}

macro_rules! skip_or_continue {
    () => {
        if let Some(reason) = skip_reason() {
            eprintln!("Skipping: {reason}");
            return Ok(());
        }
    };
}

async fn new_page() -> anyhow::Result<(Playwright, playwright_rs::protocol::browser::Browser, Page)> {
    let playwright = Playwright::launch().await?;
    let browser = playwright.chromium().launch_with_options(noscura_e2e::support::config::launch_options()).await?;
    let context = browser.new_context().await?;
    let page = context.new_page().await?;
    Ok((playwright, browser, page))
}

// Goes on to Save and Finish Appointment: the TS version of this suite found that step got
// stuck in a permanent loading spinner on this environment and stopped short of it -- kept
// here to see directly whether that's still the case.
#[tokio::test]
async fn staff_can_add_a_medicine_and_an_imaging_test_during_a_consultation() -> anyhow::Result<()> {
    skip_or_continue!();
    let (_playwright, browser, page) = new_page().await?;

    auth::login(&page).await?;
    let (date_label, slot_label) = book_appointment(&page, EXISTING_PATIENT_NAME, AppointmentMode::Offline).await?;

    if let Err(e) = enter_appointment(&page, EXISTING_PATIENT_NAME, &date_label, &slot_label).await {
        let bytes = page.screenshot(None).await?;
        std::fs::write("/tmp/consultation-debug.png", bytes)?;
        eprintln!("enter_appointment failed: {e}");
        return Err(e);
    }

    if let Err(e) = add_medicine(&page, "Dolo").await {
        let bytes = page.screenshot(None).await?;
        std::fs::write("/tmp/consultation-debug.png", bytes)?;
        eprintln!("add_medicine failed: {e}");
        return Err(e);
    }
    let dolo = text_matching(&page, &Regex::new(r"(?i)^Dolo").unwrap()).await?;
    expect(dolo).to_be_visible().await?;

    // Called before add_imaging_test: both sections' "Add Test" button shares the same exact
    // name, so this needs to run while Imaging Orders is still collapsed.
    if let Err(e) = add_investigation(&page, "CBC").await {
        let bytes = page.screenshot(None).await?;
        std::fs::write("/tmp/consultation-debug.png", bytes)?;
        eprintln!("add_investigation failed: {e}");
        return Err(e);
    }
    let cbc = text_matching(&page, &Regex::new(r"(?i)^CBC$").unwrap()).await?;
    expect(cbc).to_be_visible().await?;

    // Test Name accepts free text directly (no catalog suggestion needs to be selected, unlike
    // the medicine field above) -- this doesn't need to be a real imaging modality.
    if let Err(e) = add_imaging_test(&page, "General Imaging Scan", "Chest").await {
        let bytes = page.screenshot(None).await?;
        std::fs::write("/tmp/consultation-debug.png", bytes)?;
        eprintln!("add_imaging_test failed: {e}");
        return Err(e);
    }
    let imaging_test = text_matching(&page, &Regex::new(r"(?i)^General Imaging Scan$").unwrap()).await?;
    expect(imaging_test).to_be_visible().await?;

    if let Err(e) = save_and_finish_appointment(&page).await {
        let bytes = page.screenshot(None).await?;
        std::fs::write("/tmp/consultation-debug.png", bytes)?;
        eprintln!("save_and_finish_appointment failed: {e}");
        return Err(e);
    }

    browser.close().await?;
    Ok(())
}
