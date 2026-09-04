// To run: RUN_NOSCURA_LOGIN_SMOKE=1 LOGIN_EMAIL=<email> LOGIN_PASSWORD=<password> cargo test --test inpatient_admission -- --test-threads=1
use noscura_e2e::support::auth;
use noscura_e2e::support::config;
use noscura_e2e::support::inpatient::{admit_patient, discharge_patient, open_patient_from_dashboard_list, submit_final_bill};
use playwright_rs::expect;
use playwright_rs::protocol::locator::{AriaRole, GetByRoleOptions};
use playwright_rs::protocol::page::Page;
use playwright_rs::protocol::playwright::Playwright;

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

#[tokio::test]
async fn staff_can_create_a_patient_and_admit_them_as_an_inpatient() -> anyhow::Result<()> {
    skip_or_continue!();
    let (_playwright, browser, page) = new_page().await?;

    auth::login(&page).await?;
    let patient_name = admit_patient(&page, "Emergency Contact Person").await?;

    // Submitting creates a new row at the top of the Inpatient dashboard's patient list.
    let row = page.get_by_role(AriaRole::Row, Some(GetByRoleOptions::default().name(&patient_name).exact(false))).first();
    expect(row.clone()).with_timeout(std::time::Duration::from_secs(15)).to_be_visible().await?;

    // Admissions here are one-way with no discharge flow other than this: bill and discharge
    // the patient at the end of the run rather than leaving them admitted, so the fixed test
    // patient (EXISTING_PATIENT_NAME) is free to be admitted again on the next run.
    open_patient_from_dashboard_list(&page, &patient_name).await?;
    submit_final_bill(&page, "Kishan D").await?;
    discharge_patient(&page).await?;

    browser.close().await?;
    Ok(())
}
