import { expect, type Page } from '@playwright/test';
import { clickCenter, typeInto } from './flutter';

// The task list defaults to an unfiltered, paginated view ordered in a way that doesn't
// prioritize today or "just booked" -- confirmed live: leftover appointments from past runs,
// dated days apart, filled the entire visible first page while a same-day appointment booked
// moments earlier was nowhere on it. Filtering via the list's own search box narrows it down
// to this patient specifically, sidestepping that pagination/ordering entirely.
//
// Entering an appointment that hasn't been opened before shows a Symptoms/Allergies/Vitals
// intake dialog first; one already entered skips straight to the consultation screen. Both
// land on the same /appointmentHostHAdmin screen, so handle either.
export async function enterAppointment(page: Page, patientName: string) {
  const taskSearchBox = page.getByRole('textbox', { name: /search tasks by patient name/i }).first();
  await expect(taskSearchBox).toBeVisible({ timeout: 15000 });
  await typeInto(page, taskSearchBox, patientName);

  const row = page.getByRole('row', { name: new RegExp(patientName, 'i') }).first();
  await expect(row).toBeVisible({ timeout: 15000 });
  await clickCenter(page, row.getByRole('button').nth(1));

  const symptomsInput = page.getByRole('textbox', { name: /enter symptoms/i }).first();
  if (await symptomsInput.isVisible({ timeout: 8000 }).catch(() => false)) {
    await typeInto(page, symptomsInput, 'cough');
    await clickCenter(page, page.getByRole('button', { name: /^save$/i }).first());
  }
  await page.waitForURL(/appointmentHostHAdmin/, { timeout: 15000 });
}

// The medicine field is an autocomplete: typing free text alone is rejected ("Please select
// medicine") — a suggestion has to actually be clicked. The exact suggestion list varies
// (e.g. "Dolo 500 Tablet" vs "Dolo Drops" depending on what's in stock), so match whichever
// suggestion starts with the search term rather than a hardcoded full name. Adding also
// requires dosage or instructions to be filled in ("Please add dosage or instructions"
// otherwise).
export async function addMedicine(page: Page, searchTerm: string) {
  await clickCenter(page, page.getByRole('button', { name: /^medications$/i }).first());

  const medicineInput = page.getByRole('textbox', { name: /eg\. ascoril/i }).first();
  await typeInto(page, medicineInput, searchTerm);

  const suggestion = page.getByRole('button', { name: new RegExp(`^${searchTerm}`, 'i') }).first();
  await expect(suggestion).toBeVisible({ timeout: 10000 });
  await clickCenter(page, suggestion);

  // Selecting a suggestion re-renders the dosage/instructions fields (same class of race as
  // the online booking mode switch), so the first click can land before the new field is
  // actually interactive. Wait for the field itself to be stable before typing.
  const instructionsInput = page.getByRole('textbox', { name: /take after food/i }).first();
  await expect(instructionsInput).toBeVisible({ timeout: 10000 });
  await page.waitForTimeout(500);
  await typeInto(page, instructionsInput, 'After food');

  const addMedButton = page.getByRole('button', { name: /^add med$/i }).first();
  await addMedButton.scrollIntoViewIfNeeded();
  await page.waitForTimeout(500);
  await clickCenter(page, addMedButton);
  await expect(page.getByText(/^Medicines$/i).first()).toBeVisible({ timeout: 10000 });
}

// Unlike the medicine field, Test Name accepts free text directly — no suggestion needs to
// be selected.
export async function addImagingTest(page: Page, testName: string, bodyPart: string) {
  // The Medicines list just added above pushes every section below it (including this
  // toggle) further down the page, so its position can still be settling when we go to
  // click it — confirmed live: the click missed and Imaging Orders never expanded. Scroll
  // it into view and let the layout settle first.
  const imagingToggle = page.getByRole('button', { name: /^imaging orders$/i }).first();
  await imagingToggle.scrollIntoViewIfNeeded();
  await page.waitForTimeout(500);
  await clickCenter(page, imagingToggle);

  const testNameInput = page.getByRole('textbox', { name: /search test name/i }).first();
  await typeInto(page, testNameInput, testName);

  const bodyPartInput = page.getByRole('textbox', { name: /e\.g\., chest, brain/i }).first();
  await typeInto(page, bodyPartInput, bodyPart);

  // Same class of layout-shift race as the toggle above: filling the fields can reflow the
  // section, so re-settle before clicking Add Test.
  const addTestButton = page.getByRole('button', { name: /^add test$/i }).first();
  await addTestButton.scrollIntoViewIfNeeded();
  await page.waitForTimeout(500);
  await clickCenter(page, addTestButton);
  await expect(page.getByText(/^Test Orders$/i).first()).toBeVisible({ timeout: 10000 });
}
