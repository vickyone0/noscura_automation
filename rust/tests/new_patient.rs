// To run: RUN_NOSCURA_LOGIN_SMOKE=1 LOGIN_EMAIL=<email> LOGIN_PASSWORD=<password> cargo test --test new_patient -- --test-threads=1
use noscura_e2e::support::config;
use noscura_e2e::support::flutter::{click_center, text_matching};
use noscura_e2e::support::new_patient::{
    expect_new_patient_form_visible, fill_required_new_patient_fields, goto_new_patient_form, Gender,
    NewPatientRequiredDetails,
};
use noscura_e2e::support::{auth, flutter::wait_for_url_matching};
use playwright_rs::expect;
use playwright_rs::protocol::locator::{AriaRole, GetByRoleOptions};
use playwright_rs::protocol::page::Page;
use playwright_rs::protocol::playwright::Playwright;
use regex::Regex;
use std::time::Duration;

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

// `Playwright::launch()`'s return value owns the driver connection -- dropping it (e.g. by
// only returning `browser`/`page` from a helper and letting the `Playwright` local go out of
// scope) closes the channel out from under still-referenced browser/page handles (confirmed
// live: every call failed instantly with "Channel closed unexpectedly"). Keep all three alive
// together for the caller's whole test.
async fn new_page() -> anyhow::Result<(Playwright, playwright_rs::protocol::browser::Browser, Page)> {
    let playwright = Playwright::launch().await?;
    let browser = playwright.chromium().launch_with_options(noscura_e2e::support::config::launch_options()).await?;
    let context = browser.new_context().await?;
    let page = context.new_page().await?;
    Ok((playwright, browser, page))
}

#[tokio::test]
async fn staff_can_open_the_new_patient_page_and_start_patient_creation() -> anyhow::Result<()> {
    skip_or_continue!();
    let (_playwright, browser, page) = new_page().await?;

    auth::login(&page).await?;
    goto_new_patient_form(&page).await?;
    expect_new_patient_form_visible(&page).await?;

    browser.close().await?;
    Ok(())
}

#[tokio::test]
async fn new_patient_creation_shows_required_field_validation_for_a_blank_submit() -> anyhow::Result<()> {
    skip_or_continue!();
    let (_playwright, browser, page) = new_page().await?;

    auth::login(&page).await?;
    goto_new_patient_form(&page).await?;

    let submit = page.get_by_role(AriaRole::Button, Some(GetByRoleOptions::default().name("Submit").exact(true)));
    click_center(&page, &submit).await?;

    wait_for_url_matching(&page, &Regex::new(r"/addPatient(?:$|[?#])").unwrap(), Duration::from_secs(10)).await?;
    let heading = text_matching(&page, &Regex::new(r"(?i)^Create New Patient$").unwrap()).await?;
    expect(heading).to_be_visible().await?;
    let field_required = text_matching(&page, &Regex::new(r"(?i)Field is required").unwrap()).await?;
    expect(field_required).to_be_visible().await?;

    browser.close().await?;
    Ok(())
}

#[tokio::test]
async fn staff_can_enter_new_patient_details_and_cancel_without_creating_a_record() -> anyhow::Result<()> {
    skip_or_continue!();
    let (_playwright, browser, page) = new_page().await?;

    auth::login(&page).await?;
    goto_new_patient_form(&page).await?;

    let suffix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis();
    let details = NewPatientRequiredDetails::new(
        format!("Automation Patient {}", letters_from(suffix)),
        format!("AUTO{suffix}"),
        format!("9000{}", &suffix.to_string()[suffix.to_string().len().saturating_sub(6)..]),
    );
    fill_required_new_patient_fields(&page, &details).await?;

    let cancel = page.get_by_role(AriaRole::Button, Some(GetByRoleOptions::default().name("Cancel").exact(true)));
    click_center(&page, &cancel).await?;

    wait_for_url_matching(
        &page,
        &Regex::new(r"/newHomeAdmin\?selectedOption=1(?:$|[&#])").unwrap(),
        Duration::from_secs(15),
    )
    .await?;
    let patients_list = text_matching(&page, &Regex::new(r"(?i)Patients List").unwrap()).await?;
    expect(patients_list).with_timeout(Duration::from_secs(15)).to_be_visible().await?;
    let new_patient_button = page.get_by_role(AriaRole::Button, Some(GetByRoleOptions::default().name("New Patient").exact(false)));
    expect(new_patient_button).to_be_visible().await?;

    browser.close().await?;
    Ok(())
}

struct PatientVariant {
    title: &'static str,
    gender: Gender,
    age: &'static str,
    address: &'static str,
}

#[tokio::test]
async fn new_patient_form_variants() -> anyhow::Result<()> {
    skip_or_continue!();

    let variants = [
        PatientVariant {
            title: "female patient with a young age",
            gender: Gender::Female,
            age: "1",
            address: "Flat 12, 4th Main Road, Bengaluru",
        },
        PatientVariant {
            title: "other-gender patient with an older age",
            gender: Gender::Other,
            age: "99",
            address: "Automation test address with landmark near main reception",
        },
    ];

    for variant in variants {
        eprintln!("variant: {}", variant.title);
        let (_playwright, browser, page) = new_page().await?;
        auth::login(&page).await?;
        goto_new_patient_form(&page).await?;

        let suffix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis();
        let mut details = NewPatientRequiredDetails::new(
            format!("Automation Mixed {}", letters_from(suffix)),
            format!("NC-AUTO{suffix}"),
            format!("9100{}", &suffix.to_string()[suffix.to_string().len().saturating_sub(6)..]),
        );
        details.gender = Some(variant.gender);
        details.age = Some(variant.age.to_string());
        details.address = Some(variant.address.to_string());
        fill_required_new_patient_fields(&page, &details).await?;

        let cancel = page.get_by_role(AriaRole::Button, Some(GetByRoleOptions::default().name("Cancel").exact(true)));
        click_center(&page, &cancel).await?;

        wait_for_url_matching(
            &page,
            &Regex::new(r"/newHomeAdmin\?selectedOption=1(?:$|[&#])").unwrap(),
            Duration::from_secs(15),
        )
        .await?;
        let patients_list = text_matching(&page, &Regex::new(r"(?i)Patients List").unwrap()).await?;
        expect(patients_list).with_timeout(Duration::from_secs(15)).to_be_visible().await?;

        browser.close().await?;
    }

    Ok(())
}

fn letters_from(seed: u128) -> String {
    let mut n = seed;
    (0..8)
        .map(|_| {
            let c = (b'a' + (n % 26) as u8) as char;
            n /= 26;
            c
        })
        .collect()
}
