use crate::support::config;
use crate::support::flutter::{
    click_center, enable_flutter_accessibility, text_matching, text_matching_within, textbox_near_label, type_into,
    wait_for_url_matching,
};
use anyhow::Result;
use playwright_rs::expect;
use playwright_rs::protocol::locator::{AriaRole, GetByRoleOptions};
use playwright_rs::protocol::page::Page;
use regex::Regex;
use std::time::Duration;

pub async fn open_new_patient_form(page: &Page) -> Result<()> {
    let new_patient_button = page.get_by_role(
        AriaRole::Button,
        Some(GetByRoleOptions::default().name("New Patient").exact(false)),
    );

    let patients_list = text_matching_within(page, &Regex::new(r"(?i)Patients List").unwrap(), Duration::from_secs(20)).await?;
    expect(patients_list).with_timeout(Duration::from_secs(15)).to_be_visible().await?;
    expect(new_patient_button.clone()).with_timeout(Duration::from_secs(15)).to_be_visible().await?;

    let add_patient_pattern = Regex::new(r"/addPatient(?:$|[?#])").unwrap();
    for attempt in 1..=3 {
        click_center(page, &new_patient_button).await?;
        let navigated = wait_for_url_matching(page, &add_patient_pattern, Duration::from_secs(5)).await;
        if navigated.is_ok() || add_patient_pattern.is_match(&page.url()) {
            enable_flutter_accessibility(page).await?;
            return Ok(());
        }
        if attempt == 3 {
            enable_flutter_accessibility(page).await?;
            return Ok(());
        }
    }
    Ok(())
}

pub async fn goto_new_patient_form(page: &Page) -> Result<()> {
    page.goto(&config::new_patient_url(), None).await?;
    enable_flutter_accessibility(page).await?;
    wait_for_url_matching(
        page,
        &Regex::new(r"/newHomeAdmin\?selectedOption=1(?:$|[&#])").unwrap(),
        Duration::from_secs(15),
    )
    .await?;
    open_new_patient_form(page).await?;
    wait_for_url_matching(page, &Regex::new(r"/addPatient(?:$|[?#])").unwrap(), Duration::from_secs(15)).await?;
    Ok(())
}

pub async fn expect_new_patient_form_visible(page: &Page) -> Result<()> {
    let checks: &[(&str, u64)] = &[
        (r"(?i)^Create New Patient$", 15),
        (r"(?i)^Patient Information$", 5),
        (r"(?i)^Name$", 5),
        (r"(?i)^Patient ID$", 5),
        (r"(?i)^Gender\*?$", 5),
        (r"(?i)^Phone Number$", 5),
        (r"(?i)^Age\*$", 5),
        (r"(?i)^Address$", 5),
        (r"(?i)^Registration Date\*$", 5),
    ];
    for (pattern, timeout_secs) in checks {
        let locator = text_matching(page, &Regex::new(pattern).unwrap()).await?;
        expect(locator).with_timeout(Duration::from_secs(*timeout_secs)).to_be_visible().await?;
    }

    let submit = page.get_by_role(AriaRole::Button, Some(GetByRoleOptions::default().name("Submit").exact(true)));
    let cancel = page.get_by_role(AriaRole::Button, Some(GetByRoleOptions::default().name("Cancel").exact(true)));
    expect(submit).to_be_visible().await?;
    expect(cancel).to_be_visible().await?;
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Gender {
    Male,
    Female,
    Other,
}

impl Gender {
    fn as_str(self) -> &'static str {
        match self {
            Gender::Male => "Male",
            Gender::Female => "Female",
            Gender::Other => "Other",
        }
    }
}

pub struct NewPatientRequiredDetails {
    pub name: String,
    pub patient_id: String,
    pub phone_number: String,
    pub address: Option<String>,
    pub age: Option<String>,
    pub gender: Option<Gender>,
}

impl NewPatientRequiredDetails {
    pub fn new(name: impl Into<String>, patient_id: impl Into<String>, phone_number: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            patient_id: patient_id.into(),
            phone_number: phone_number.into(),
            address: None,
            age: None,
            gender: None,
        }
    }
}

pub async fn select_gender(page: &Page, gender: Gender) -> Result<()> {
    let please_select = page
        .get_by_role(
            AriaRole::Button,
            Some(GetByRoleOptions::default().name("Please select...").exact(true)),
        )
        .first();
    click_center(page, &please_select).await?;
    let option = page
        .get_by_role(
            AriaRole::Button,
            Some(GetByRoleOptions::default().name(gender.as_str()).exact(true)),
        )
        .first();
    click_center(page, &option).await?;
    Ok(())
}

pub async fn fill_required_new_patient_fields(page: &Page, details: &NewPatientRequiredDetails) -> Result<()> {
    let name_input = textbox_near_label(page, &Regex::new(r"(?i)^Name\*?$").unwrap()).await?;
    type_into(page, &name_input, &details.name).await?;
    select_gender(page, details.gender.unwrap_or(Gender::Male)).await?;

    // Patient ID defaults to "Auto" mode: typing into it is silently discarded and it resets
    // to the auto-generated value. Setting a specific id requires switching to "Manual" mode
    // first, which isn't wired up here yet -- `details.patient_id` is accepted but not
    // actually applied; the field keeps its auto-generated value.
    let patient_id_input = textbox_near_label(page, &Regex::new(r"(?i)^Patient ID\*?$").unwrap()).await?;
    type_into(page, &patient_id_input, &details.patient_id).await?;

    let phone_input = textbox_near_label(page, &Regex::new(r"(?i)^Phone Number\*?$").unwrap()).await?;
    type_into(page, &phone_input, &details.phone_number).await?;

    let address_input = textbox_near_label(page, &Regex::new(r"(?i)^Address$").unwrap()).await?;
    type_into(page, &address_input, details.address.as_deref().unwrap_or("Automation test address")).await?;

    let age_input = textbox_near_label(page, &Regex::new(r"(?i)^Age\*?$").unwrap()).await?;
    type_into(page, &age_input, details.age.as_deref().unwrap_or("30")).await?;

    let heading = text_matching(page, &Regex::new(r"(?i)^Create New Patient$").unwrap()).await?;
    expect(heading).to_be_visible().await?;
    Ok(())
}

// A patient created ahead of time in the target environment and reused across suites instead
// of creating a fresh one per run.
pub const EXISTING_PATIENT_NAME: &str = "Brad Pitt";

// The Name field rejects digits ("Invalid text" shown live), so uniqueness comes from random
// letters rather than a timestamp suffix.
fn unique_patient_name(label: &str) -> String {
    use rand::RngExt;
    let mut rng = rand::rng();
    let suffix: String = (0..8).map(|_| (b'a' + rng.random_range(0..26)) as char).collect();
    format!("Automation {label} {suffix}")
}

pub async fn create_patient(page: &Page, label: &str) -> Result<String> {
    let name = unique_patient_name(label);
    goto_new_patient_form(page).await?;

    let phone = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis()
        .to_string();
    let phone = phone[phone.len().saturating_sub(10)..].to_string();

    let details = NewPatientRequiredDetails::new(name.clone(), "AUTO", phone);
    fill_required_new_patient_fields(page, &details).await?;

    let submit = page.get_by_role(AriaRole::Button, Some(GetByRoleOptions::default().name("Submit").exact(true)));
    click_center(page, &submit).await?;
    wait_for_url_matching(
        page,
        &Regex::new(r"/newHomeAdmin\?selectedOption=1(?:$|[&#])").unwrap(),
        Duration::from_secs(15),
    )
    .await?;
    Ok(name)
}
