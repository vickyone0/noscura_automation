// To run: RUN_NOSCURA_LOGIN_SMOKE=1 LOGIN_EMAIL=<email> LOGIN_PASSWORD=<password> cargo test --test login -- --test-threads=1
use noscura_e2e::support::auth::open_login_form;
use noscura_e2e::support::config;
use noscura_e2e::support::flutter::{click_center, text_matching_any, type_into};
use playwright_rs::expect_page;
use playwright_rs::protocol::playwright::Playwright;

/// Returns Some(reason) if this smoke test should be skipped, mirroring the TS suite's
/// `test.skip(...)` guards (RUN_NOSCURA_LOGIN_SMOKE / LOGIN_EMAIL / LOGIN_PASSWORD).
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

#[tokio::test]
async fn user_can_log_in_with_a_valid_email_and_password() -> anyhow::Result<()> {
    skip_or_continue!();

    let playwright = Playwright::launch().await?;
    let browser = playwright.chromium().launch_with_options(noscura_e2e::support::config::launch_options()).await?;
    let context = browser.new_context().await?;
    let page = context.new_page().await?;

    let form = open_login_form(&page).await?;
    type_into(&page, &form.email_input, &config::login_email().unwrap()).await?;
    type_into(&page, &form.password_input, &config::login_password().unwrap()).await?;
    noscura_e2e::support::auth::submit_and_wait_for_navigation(&page, &form.submit_button).await?;

    expect_page(&page).not().to_have_url_regex(r"/login(?:$|[?#])").await?;

    browser.close().await?;
    Ok(())
}

#[tokio::test]
async fn login_is_rejected_for_a_badly_formatted_email() -> anyhow::Result<()> {
    skip_or_continue!();

    let playwright = Playwright::launch().await?;
    let browser = playwright.chromium().launch_with_options(noscura_e2e::support::config::launch_options()).await?;
    let context = browser.new_context().await?;
    let page = context.new_page().await?;

    let form = open_login_form(&page).await?;
    type_into(&page, &form.email_input, "not-an-email").await?;
    type_into(&page, &form.password_input, &config::login_password().unwrap()).await?;
    click_center(&page, &form.submit_button).await?;

    let error = text_matching_any(&page, &["badly formatted"], std::time::Duration::from_secs(10)).await?;
    playwright_rs::expect(error).to_be_visible().await?;
    expect_page(&page).to_have_url_regex(r"/login(?:$|[?#])").await?;

    browser.close().await?;
    Ok(())
}

#[tokio::test]
async fn login_is_rejected_for_a_well_formed_but_unregistered_email() -> anyhow::Result<()> {
    skip_or_continue!();

    let playwright = Playwright::launch().await?;
    let browser = playwright.chromium().launch_with_options(noscura_e2e::support::config::launch_options()).await?;
    let context = browser.new_context().await?;
    let page = context.new_page().await?;

    let form = open_login_form(&page).await?;
    let unregistered_email = format!(
        "does-not-exist-{}@noscura.in",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis()
    );
    type_into(&page, &form.email_input, &unregistered_email).await?;
    type_into(&page, &form.password_input, &config::login_password().unwrap()).await?;
    click_center(&page, &form.submit_button).await?;

    let error = text_matching_any(
        &page,
        &["incorrect", "malformed", "expired", "not found", "no user"],
        std::time::Duration::from_secs(10),
    )
    .await?;
    playwright_rs::expect(error).to_be_visible().await?;
    expect_page(&page).to_have_url_regex(r"/login(?:$|[?#])").await?;

    browser.close().await?;
    Ok(())
}
