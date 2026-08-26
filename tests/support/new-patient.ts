import { expect, type Page } from '@playwright/test';
import { newPatientURL } from './config';
import { clickCenter, enableFlutterAccessibility, textboxNearLabel, typeInto } from './flutter';

export async function openNewPatientForm(page: Page) {
  const newPatientButton = page.getByRole('button', { name: /^(create new patient|new patient)$/i }).first();

  await expect(page.getByText(/Patients List/i).first()).toBeVisible({ timeout: 15000 });
  await expect(newPatientButton).toBeVisible({ timeout: 15000 });

  for (let attempt = 1; attempt <= 3; attempt++) {
    await clickCenter(page, newPatientButton);
    try {
      await page.waitForURL(/\/addPatient(?:$|[?#])/, { timeout: 5000 });
      await enableFlutterAccessibility(page);
      return;
    } catch {
      if (attempt === 3 || /\/addPatient(?:$|[?#])/.test(new URL(page.url()).pathname)) {
        await enableFlutterAccessibility(page);
        return;
      }
    }
  }
}

export async function gotoNewPatientForm(page: Page) {
  await page.goto(newPatientURL);
  await enableFlutterAccessibility(page);
  await expect(page).toHaveURL(/\/newHomeAdmin\?selectedOption=1(?:$|[&#])/);
  await openNewPatientForm(page);
  await expect(page).toHaveURL(/\/addPatient(?:$|[?#])/);
}

export async function expectNewPatientFormVisible(page: Page) {
  await expect(page.getByText(/^Create New Patient$/i).first()).toBeVisible({ timeout: 15000 });
  await expect(page.getByText(/^Patient Information$/i).first()).toBeVisible();
  await expect(page.getByText(/^Name$/i).first()).toBeVisible();
  await expect(page.getByText(/^Patient ID$/i).first()).toBeVisible();
  await expect(page.getByText(/^Gender\*?$/i).first()).toBeVisible();
  await expect(page.getByText(/^Phone Number$/i).first()).toBeVisible();
  await expect(page.getByText(/^Age\*$/i).first()).toBeVisible();
  await expect(page.getByText(/^Address$/i).first()).toBeVisible();
  await expect(page.getByText(/^Registration Date\*$/i).first()).toBeVisible();
  await expect(page.getByRole('button', { name: /^submit$/i }).first()).toBeVisible();
  await expect(page.getByRole('button', { name: /^cancel$/i }).first()).toBeVisible();
}

export type Gender = 'Male' | 'Female' | 'Other';

export type NewPatientRequiredDetails = {
  name: string;
  patientId: string;
  phoneNumber: string;
  address?: string;
  age?: string;
  gender?: Gender;
};

export async function selectGender(page: Page, gender: Gender) {
  await clickCenter(page, page.getByRole('button', { name: /^please select\.\.\.$/i }).first());
  await clickCenter(page, page.getByRole('button', { name: new RegExp(`^${gender}$`, 'i') }).first());
}

export async function fillRequiredNewPatientFields(page: Page, details: NewPatientRequiredDetails) {
  await typeInto(page, await textboxNearLabel(page, /^Name\*?$/i), details.name);
  await selectGender(page, details.gender ?? 'Male');

  // Patient ID defaults to "Auto" mode: typing into it is silently discarded and it resets
  // to the auto-generated value (confirmed live via inputValue() — not a locator bug).
  // Setting a specific id requires switching to "Manual" mode first, which isn't wired up
  // here yet — its radio toggle has no accessible label in this app's semantics tree, so it
  // needs a coordinate-based click. Until that's added, `details.patientId` is accepted but
  // not actually applied; the field keeps its auto-generated value.
  await typeInto(page, await textboxNearLabel(page, /^Patient ID\*?$/i), details.patientId);

  await typeInto(page, await textboxNearLabel(page, /^Phone Number\*?$/i), details.phoneNumber);
  await typeInto(page, await textboxNearLabel(page, /^Address$/i), details.address ?? 'Automation test address');
  await typeInto(page, await textboxNearLabel(page, /^Age\*?$/i), details.age ?? '30');

  await expect(page.getByText(/^Create New Patient$/i).first()).toBeVisible();
}

// Several other flows (inpatient admission, appointment booking, consultations) need a
// patient that's guaranteed not to already be tangled up in some other flow's state from a
// prior run (an existing admission, a pending task, an already-booked slot) -- confirmed live
// as a real, recurring problem: reusing one fixed "test patient" name across many spec files
// eventually collides with whatever state a previous run left that patient in. Creating a
// fresh one per run sidesteps that entirely. The Name field rejects digits ("Invalid text"
// shown live), so uniqueness comes from random letters rather than a timestamp suffix.
function uniquePatientName(label: string): string {
  let suffix = '';
  for (let i = 0; i < 8; i++) suffix += String.fromCharCode(97 + Math.floor(Math.random() * 26));
  return `Automation ${label} ${suffix}`;
}

export async function createPatient(page: Page, label = 'Patient'): Promise<string> {
  const name = uniquePatientName(label);
  await gotoNewPatientForm(page);
  await fillRequiredNewPatientFields(page, {
    name,
    patientId: 'AUTO', // Patient ID defaults to auto-generated mode; this is discarded.
    phoneNumber: String(Date.now()).slice(-10),
  });
  await clickCenter(page, page.getByRole('button', { name: /^submit$/i }).first());
  await page.waitForURL(/\/newHomeAdmin\?selectedOption=1(?:$|[&#])/, { timeout: 15000 });
  return name;
}
