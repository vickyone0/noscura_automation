use crate::support::config;
use crate::support::flutter::{click_center, enable_flutter_accessibility, type_into};
use anyhow::{anyhow, Result};
use playwright_rs::protocol::locator::{AriaRole, GetByRoleOptions, Locator};
use playwright_rs::protocol::page::Page;
use playwright_rs::expect;
use std::time::Duration;

pub struct LoginForm {
    pub email_input: Locator,
    pub password_input: Locator,
    pub submit_button: Locator,
}

pub async fn open_login_form(page: &Page) -> Result<LoginForm> {
    page.goto(&config::login_url(), None).await?;
    enable_flutter_accessibility(page).await?;

    let email_input = page.get_by_role(
        AriaRole::Textbox,
        Some(GetByRoleOptions::default().name("Enter your email").exact(false)),
    );
    let password_input = page.locator("input[type=\"password\"]");
    let submit_button = page.get_by_role(
        AriaRole::Button,
        Some(GetByRoleOptions::default().name("Log In").exact(true)),
    );

    expect(email_input.clone()).with_timeout(Duration::from_secs(15)).to_be_visible().await?;
    expect(password_input.clone()).with_timeout(Duration::from_secs(15)).to_be_visible().await?;

    Ok(LoginForm { email_input, password_input, submit_button })
}

// Against the live site, a submit click occasionally lands a frame before the button is
// actually interactive and the tap is dropped. Retry the click rather than the whole test.
pub async fn submit_and_wait_for_navigation(page: &Page, submit_button: &Locator) -> Result<()> {
    let still_on_login = |page: &Page| page.url().contains("/login");
    for attempt in 1..=3 {
        if !still_on_login(page) {
            return Ok(());
        }
        // The page can navigate away mid-click (confirmed live: the error's own reported URL
        // was already newHomeAdmin), which surfaces as click_center erroring because the
        // button it was waiting on no longer exists -- check for that success case before
        // treating the error as real.
        if let Err(e) = click_center(page, submit_button).await {
            if !still_on_login(page) {
                return Ok(());
            }
            if attempt == 3 {
                return Err(e);
            }
            continue;
        }

        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while still_on_login(page) && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        if !still_on_login(page) {
            return Ok(());
        }
        if attempt == 3 {
            return Err(anyhow!("Did not navigate away from /login after submitting."));
        }
    }
    Ok(())
}

pub async fn login(page: &Page) -> Result<()> {
    let form = open_login_form(page).await?;
    let email = config::login_email().ok_or_else(|| anyhow!("LOGIN_EMAIL not set"))?;
    let password = config::login_password().ok_or_else(|| anyhow!("LOGIN_PASSWORD not set"))?;
    type_into(page, &form.email_input, &email).await?;
    type_into(page, &form.password_input, &password).await?;
    submit_and_wait_for_navigation(page, &form.submit_button).await?;
    Ok(())
}
