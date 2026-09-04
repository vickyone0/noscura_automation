// To run: RUN_NOSCURA_LOGIN_SMOKE=1 LOGIN_EMAIL=<email> LOGIN_PASSWORD=<password> cargo test --test lab_booking -- --test-threads=1
use noscura_e2e::support::auth;
use noscura_e2e::support::config;
use noscura_e2e::support::lab::book_lab_test;
use noscura_e2e::support::new_patient::EXISTING_PATIENT_NAME;
use playwright_rs::expect;
use playwright_rs::protocol::locator::AriaRole;
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

#[tokio::test]
async fn staff_can_search_a_patient_and_book_a_lab_test() -> anyhow::Result<()> {
    skip_or_continue!();
    let (_playwright, browser, page) = new_page().await?;

    auth::login(&page).await?;
    book_lab_test(&page, EXISTING_PATIENT_NAME, "CBC", "Kishan D").await?;

    // Submitting creates a new row in the Laboratory dashboard's task list with Origin "Lab".
    let task_table = page.get_by_role(AriaRole::Table, None).nth(1);
    expect(task_table.clone()).to_be_visible().await?;

    let row_pattern = Regex::new(&format!("(?i){}", regex::escape(EXISTING_PATIENT_NAME))).unwrap();
    let mut found = false;
    let count = task_table.get_by_role(AriaRole::Row, None).count().await?;
    for i in 0..count as i32 {
        let row = task_table.get_by_role(AriaRole::Row, None).nth(i);
        let text = row.inner_text().await.unwrap_or_default();
        if row_pattern.is_match(&text) && text.contains("Lab") {
            found = true;
            break;
        }
    }
    assert!(found, "Expected a task-list row for {EXISTING_PATIENT_NAME} with Origin \"Lab\".");

    browser.close().await?;
    Ok(())
}
