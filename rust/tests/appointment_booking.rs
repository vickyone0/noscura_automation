// To run: RUN_NOSCURA_LOGIN_SMOKE=1 LOGIN_EMAIL=<email> LOGIN_PASSWORD=<password> cargo test --test appointment_booking -- --test-threads=1
use noscura_e2e::support::appointments::{book_appointment, AppointmentMode};
use noscura_e2e::support::config;
use noscura_e2e::support::auth;
use noscura_e2e::support::new_patient::EXISTING_PATIENT_NAME;
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
async fn staff_can_search_a_patient_and_book_an_offline_appointment() -> anyhow::Result<()> {
    skip_or_continue!();
    let (_playwright, browser, page) = new_page().await?;

    auth::login(&page).await?;
    book_appointment(&page, EXISTING_PATIENT_NAME, AppointmentMode::Offline).await?;

    browser.close().await?;
    Ok(())
}

#[tokio::test]
async fn staff_can_search_a_patient_and_book_an_online_appointment() -> anyhow::Result<()> {
    skip_or_continue!();
    let (_playwright, browser, page) = new_page().await?;

    auth::login(&page).await?;
    book_appointment(&page, EXISTING_PATIENT_NAME, AppointmentMode::Online).await?;

    browser.close().await?;
    Ok(())
}

