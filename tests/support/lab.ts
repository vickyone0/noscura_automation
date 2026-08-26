import { expect, type Locator, type Page } from '@playwright/test';
import { buttonRightOf, clickCenter, textBelow, typeInto, typeIntoVerified } from './flutter';

// Searching a patient shows a single result card with an unlabeled arrow icon (no "Proceed"
// text, unlike the Outpatient booking flow) — located relative to the patient's name text
// rather than by role+name.
async function openLabBookingForPatient(page: Page, patientName: string) {
  const labTab = page.getByRole('button', { name: /^laboratory$/i }).first();
  await expect(labTab).toBeVisible({ timeout: 15000 });
  await clickCenter(page, labTab);

  // This field's accessible name becomes whatever was typed once it has a value (its aria-
  // label isn't a fixed placeholder) — a role+name locator bound to the original placeholder
  // text stops resolving after the first keystroke, breaking a retry that re-queries it the
  // same way. Target it positionally (it's the first textbox on this dashboard) so a retry
  // can find it again regardless of its current value.
  const searchBox = page.getByRole('textbox').first();
  await expect(searchBox).toBeVisible({ timeout: 15000 });
  // A dropped first character here (e.g. "Aneesh" -> "neesh") means the search matches no
  // one and the rest of the flow never finds a result card — worth the extra verification.
  await typeIntoVerified(page, searchBox, patientName);

  // The patient's name can already be showing further down in the (unfiltered, still
  // loading) task list before the debounced search even returns — a plain "does this text
  // exist yet" wait matches that immediately, not the search result card actually appearing.
  // textBelow itself needs the search-result card to already be there to succeed (it must sit
  // just below the search box, not wherever DOM order happens to put a same-named task-list
  // row), so retry it directly until a genuinely-nearby match shows up.
  let resultName!: Locator;
  await expect(async () => {
    resultName = await textBelow(page, searchBox, new RegExp(`^${patientName}$`, 'i'));
  }).toPass({ timeout: 15000 });
  const resultAction = await buttonRightOf(page, resultName);

  // This click reliably navigates in a plain script driving the same page, but not
  // consistently under the Playwright test runner against this live site (root cause
  // unclear, same unexplained gap seen elsewhere in this suite) — retry it rather than the
  // whole test.
  for (let attempt = 1; attempt <= 3; attempt++) {
    await clickCenter(page, resultAction);

    // If the patient already has a pending lab task, a dialog asks whether to view it or
    // book another one anyway — proceed with a new booking rather than getting stuck on it.
    // `isVisible()` checks immediately rather than polling, so it can catch the dialog mid
    // fade-in and report false right before it's actually there (confirmed live: a
    // screenshot taken right after a "false" check still showed the dialog on screen) — use
    // `expect`, which retries, to decide whether it's showing at all.
    const confirmButton = page.getByRole('button', { name: /^confirm$/i }).first();
    const dialogShown = await expect(confirmButton)
      .toBeVisible({ timeout: 5000 })
      .then(() => true)
      .catch(() => false);
    if (dialogShown) {
      await clickCenter(page, confirmButton);
    }

    const navigated = await page
      .waitForURL(/serviceDetailsLab/, { timeout: 5000 })
      .then(() => true)
      .catch(() => false);
    if (navigated) return;
    if (attempt === 3) throw new Error(`Did not navigate to the lab booking form after ${attempt} attempts.`);
  }
}

// The Test Name field is an autocomplete, same shape as the medicine field in the
// consultation flow: typing alone doesn't add anything, a suggestion has to be selected.
// Selecting one auto-fills Description and Amount from the catalog entry.
async function addLabTest(page: Page, testName: string) {
  const testNameInput = page.getByRole('textbox', { name: /e\.g\., : cbc/i }).first();
  await expect(testNameInput).toBeVisible({ timeout: 10000 });

  await typeInto(page, testNameInput, testName);

  const suggestion = page.getByRole('button', { name: new RegExp(`^${testName}$`, 'i') }).first();
  await expect(suggestion).toBeVisible({ timeout: 10000 });
  await clickCenter(page, suggestion);

  // Selecting a suggestion triggers Description/Amount to populate asynchronously (same class
  // of re-render race as the medicine suggestion in the consultation flow) — buttonRightOf
  // iterates every button on the page, and hitting that mid-churn can stall on a stale
  // reference, or land on the wrong button if the row's position drifted since typing began.
  // Let the fields finish settling first, then anchor off the "Amount*" label, which keeps
  // both a fixed name and a fixed position (unlike the Test Name/Amount fields themselves,
  // which switch from textbox to button-like display once a suggestion is selected).
  await page.waitForTimeout(1000);
  const amountLabel = page.getByText(/^Amount\*$/i).first();
  await expect(amountLabel).toBeVisible({ timeout: 10000 });
  const amountLabelBox = await amountLabel.boundingBox();
  if (!amountLabelBox) throw new Error('Could not read a bounding box for the Amount label.');

  // The "+" action button that commits this row to the billing table below is unlabeled —
  // it sits to the right of the Amount field/label, on the same row.
  const addButton = await buttonRightOf(page, amountLabelBox);
  await clickCenter(page, addButton);

  // Confirm the row actually landed in the billing table before moving on, rather than
  // discovering a missed click much later at Submit time. Flutter merges the whole payment
  // summary into one text node ("Total Amount : ... Rs. 250 Amount Test Name ..."), so match
  // the amount as a substring rather than anchoring to the start of the text.
  await expect(page.getByText(/Rs\.\s*250/i).first()).toBeVisible({ timeout: 10000 });
}

export async function bookLabTest(page: Page, patientName: string, testName: string, doctorName: string) {
  await openLabBookingForPatient(page, patientName);
  await addLabTest(page, testName);

  const doctorInput = page.getByRole('textbox', { name: /eg\. john doe/i }).first();
  await typeInto(page, doctorInput, doctorName);

  const submitButton = page.getByRole('button', { name: /^submit$/i }).first();
  await clickCenter(page, submitButton);

  // Submitting redirects back to the Laboratory dashboard's task list.
  await page.waitForURL(/newHomeAdmin/, { timeout: 15000 });
}
