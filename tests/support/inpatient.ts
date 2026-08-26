import { expect, type Locator, type Page } from '@playwright/test';
import {
  clickCenter,
  dropdownNearLabel,
  selectDropdownOption,
  selectFirstDropdownOption,
  typeInto,
  typeIntoVerified,
} from './flutter';
import { createPatient } from './new-patient';

// Admitting a patient who already has an active admission sends the search result to a "view
// admission" page instead of the admit form (confirmed live: it lands on /serviceDetailsIP
// instead of /admitPatient) -- and admissions here are one-way, with no discharge flow this
// suite exercises to free a patient back up. Creating a fresh, never-before-admitted patient
// for every run avoids that conflict entirely.

// The search result is one wide card; the only element inside it with its own accessible name
// is a small unlabeled chevron icon, but clicking that precise ~24x24 icon doesn't reliably
// register a click (confirmed live: retried for 6+ seconds with the page never navigating).
// The card's own outer area is a separate, much larger button covering the whole row, and
// clicking that navigates reliably -- so target the widest button that renders just below the
// search box rather than the icon inside it.
async function openAdmissionFormForPatient(page: Page, patientName: string) {
  const inpatientTab = page.getByRole('button', { name: /^inpatient$/i }).first();
  await expect(inpatientTab).toBeVisible({ timeout: 15000 });
  await clickCenter(page, inpatientTab);

  const searchBox = page.getByRole('textbox').first();
  await expect(searchBox).toBeVisible({ timeout: 15000 });
  await typeIntoVerified(page, searchBox, patientName);

  let resultRow!: Locator;
  await expect(async () => {
    const searchBoxBox = await searchBox.boundingBox();
    if (!searchBoxBox) throw new Error('Search box is not ready yet.');

    const buttons = page.getByRole('button');
    const count = await buttons.count();
    let found = -1;
    for (let i = 0; i < count; i++) {
      const box = await buttons.nth(i).boundingBox();
      if (!box) continue;
      const verticalGap = box.y - (searchBoxBox.y + searchBoxBox.height);
      if (verticalGap > 0 && verticalGap < 150 && box.width > 500) found = i;
    }
    if (found === -1) throw new Error('Search result row has not rendered yet.');
    resultRow = buttons.nth(found);
  }).toPass({ timeout: 15000 });

  await clickCenter(page, resultRow);
  await page.waitForURL(/admitPatient/, { timeout: 15000 });
}

// Department, Doctor, and Emergency Contact Full Name are required fields on this form.
async function fillRequiredAdmissionDetails(page: Page, emergencyContactName: string) {
  const deptDropdown = await dropdownNearLabel(page, /^Department\*$/i);
  await selectDropdownOption(page, deptDropdown, /^General Medicine$/i);

  // The Doctor list is scoped to whichever Department was just picked, so it must be
  // re-located fresh rather than assumed to be at a fixed position.
  const doctorDropdown = await dropdownNearLabel(page, /^Doctor\*$/i);
  await selectDropdownOption(page, doctorDropdown, /kishan/i);

  const fullNameInput = page.getByRole('textbox', { name: /e\.g\., : john doe/i }).first();
  await typeInto(page, fullNameInput, emergencyContactName);
}

// Every Room Type this environment offers, confirmed live via its dropdown's option list.
// Male Ward is tried first since it matches the default patient gender created here; the
// rest are fallbacks purely to find an open bed, not a clinically appropriate match.
const ROOM_TYPES_TO_TRY = [/^Male Ward$/i, /^AC- Rooms$/i, /^ICU$/i, /^Non AC Room$/i, /^Female Ward$/i];

// Unlike dropdownNearLabel (which locates a control by its "Please select..." placeholder
// text), this finds a dropdown control by its label regardless of current value -- needed for
// Room Type and Floor/Block specifically, since selectRoomTypeAndBed re-opens both after a
// first pick already replaced their placeholders with a chosen value (confirmed live:
// dropdownNearLabel then finds nothing, since no "Please select..." text is left near either
// label to anchor on once something's been picked).
async function controlNearLabel(page: Page, labelPattern: RegExp): Promise<Locator> {
  const label = page.getByText(labelPattern).first();
  await label.scrollIntoViewIfNeeded();
  const labelBox = await label.boundingBox();
  if (!labelBox) throw new Error(`Could not find a label matching ${labelPattern}.`);

  const buttons = page.getByRole('button');
  const count = await buttons.count();
  let best: { index: number; distance: number } | null = null;
  for (let i = 0; i < count; i++) {
    const box = await buttons.nth(i).boundingBox();
    if (!box) continue;
    const verticalGap = box.y - labelBox.y;
    if (verticalGap < 0 || verticalGap > 60) continue;
    const horizontalGap = Math.max(0, labelBox.x - (box.x + box.width), box.x - (labelBox.x + labelBox.width));
    if (horizontalGap > 300) continue;
    const distance = verticalGap + horizontalGap;
    if (!best || distance < best.distance) best = { index: i, distance };
  }
  if (best === null) throw new Error(`Could not find a control near a label matching ${labelPattern}.`);
  return buttons.nth(best.index);
}

// Room Type -> Floor/Block -> a "Bed Manager" dialog listing individual beds (e.g.
// "WARD-M - 205 - BED1"). Repeated runs (this suite's own past ones included) permanently
// occupy beds with no discharge flow to free them, so a room type that had space yesterday
// can show "All beds are full." today (confirmed live) -- try each room type in turn rather
// than assuming any one of them still has room.
async function selectRoomTypeAndBed(page: Page) {
  for (const roomTypePattern of ROOM_TYPES_TO_TRY) {
    await selectDropdownOption(page, await controlNearLabel(page, /^Room Type\*$/i), roomTypePattern);

    // Only one Floor/Block value is configured per room type in this environment; any one
    // option is fine, so pick whichever renders rather than hardcoding it.
    await selectFirstDropdownOption(page, await controlNearLabel(page, /^Floor\/Block\*?$/i));

    const selectBed = page.getByText(/^Select Bed$/i).first();
    await expect(selectBed).toBeVisible({ timeout: 10000 });
    await clickCenter(page, selectBed);

    const bedManagerDialog = page.getByRole('dialog').filter({ hasText: 'Bed Manager' });
    await expect(bedManagerDialog).toBeVisible({ timeout: 10000 });

    const bedOption = bedManagerDialog.getByRole('button', { name: /WARD.*BED\d+/i }).first();
    const bedAvailable = await expect(bedOption)
      .toBeVisible({ timeout: 5000 })
      .then(() => true)
      .catch(() => false);

    if (bedAvailable) {
      await clickCenter(page, bedOption);
      // Picking a bed reflows the rest of the form (Advance Amount/Payment Mode shift down
      // to make room for the new "Room/Bed Number" field) -- confirmed live: reading
      // positions immediately after this click intermittently catches the pre-reflow layout.
      await page.waitForTimeout(800);
      return;
    }

    // "All beds are full." -- close the dialog via its only (unlabeled) button and try the
    // next room type. Reading that button's position occasionally stalls for several seconds
    // under the Playwright test runner despite the identical click working immediately every
    // time in a plain script against the same page (confirmed live) -- the same unexplained
    // runner-only gap seen elsewhere in this suite. Retry the close rather than let one slow
    // read fail the whole admission.
    let dialogClosed = false;
    for (let attempt = 1; attempt <= 3 && !dialogClosed; attempt++) {
      await clickCenter(page, bedManagerDialog.getByRole('button').first());
      dialogClosed = await expect(bedManagerDialog)
        .toBeHidden({ timeout: 5000 })
        .then(() => true)
        .catch(() => false);
    }
    if (!dialogClosed) throw new Error('Could not close the full "Bed Manager" dialog.');
  }
  throw new Error(`No room type had an available bed among ${ROOM_TYPES_TO_TRY.map(String).join(', ')}.`);
}

// Confirmed live: selecting Payment Mode after typing the Advance Amount can leave the amount
// field rendering empty again (a Flutter widget-rebuild side effect, not a real navigation or
// clear action) -- so Payment Mode is selected first, and Advance Amount is filled last, right
// before Submit, to land after any such rebuild rather than before it.
async function fillBillingDetails(page: Page, advanceAmount: string) {
  const paymentModeDropdown = await dropdownNearLabel(page, /^Payment Mode$/i);
  await selectDropdownOption(page, paymentModeDropdown, /^cash$/i);

  const advanceAmountInput = page.getByRole('textbox', { name: '00' }).first();
  await typeInto(page, advanceAmountInput, advanceAmount);
}

export async function admitPatient(page: Page, emergencyContactName: string): Promise<string> {
  const patientName = await createPatient(page, 'Inpatient');

  await openAdmissionFormForPatient(page, patientName);
  await fillRequiredAdmissionDetails(page, emergencyContactName);
  await selectRoomTypeAndBed(page);
  await fillBillingDetails(page, '500');

  const submitButton = page.getByRole('button', { name: /^submit$/i }).first();
  await clickCenter(page, submitButton);

  // Submitting redirects back to the Inpatient dashboard's patient list.
  await page.waitForURL(/newHomeAdmin/, { timeout: 15000 });

  return patientName;
}
