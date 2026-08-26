import { expect, type Locator, type Page } from '@playwright/test';
import { clickCenter, typeInto } from './flutter';

export type AppointmentMode = 'Offline' | 'Online';

// Booking a real slot creates a live appointment + payment record in the shared test
// database, so we verify success by the task list's total count growing by exactly one
// rather than matching specific text — a prior run's leftover row would otherwise
// false-positive. Rendered <row> elements can't be used directly since the list paginates
// at 5 per page; the true total lives in the "X–Y of N" pagination label instead.
export async function readTaskListTotal(page: Page): Promise<number> {
  const paginationLabel = await page.getByText(/of\s+\d+/i).first().textContent();
  const match = paginationLabel?.match(/of\s+(\d+)/i);
  if (!match) throw new Error(`Could not read task list total from "${paginationLabel}".`);
  return Number(match[1]);
}

// Neither `.first()` nor `.last()` reliably lands on a bookable slot here, for two
// independent reasons confirmed live: Flutter's semantics DOM order for the grid doesn't
// track chronological order, and — more fundamentally — bookable slots sit in a rolling
// window near the *current* time rather than "anywhere today" (9/10 AM got rejected as
// "Doctor is not available" while 11 AM, a bit under an hour after the real clock's 10:2x AM,
// succeeded immediately). So compute candidate labels directly from the current time plus a
// buffer, rounded to the grid's slot size, instead of searching blindly. This also sidesteps
// Online's full-day grid being virtualized (an exact-text lookup for a clearly-existing later
// slot returns zero matches until scrolled near it): asking for one specific computed label
// lets Playwright's own actionability scrolling bring it into view.
export function computeCandidateSlotLabels(bufferMinutes: number, stepMinutes: number, count: number): string[] {
  const now = new Date();
  const start = Math.ceil((now.getHours() * 60 + now.getMinutes() + bufferMinutes) / stepMinutes) * stepMinutes;
  const labels: string[] = [];
  for (let i = 0; i < count; i++) {
    const totalMinutes = (start + i * stepMinutes) % (24 * 60);
    const hour24 = Math.floor(totalMinutes / 60);
    const minute = totalMinutes % 60;
    const hour12 = hour24 % 12 === 0 ? 12 : hour24 % 12;
    labels.push(`${hour12}:${String(minute).padStart(2, '0')} ${hour24 < 12 ? 'AM' : 'PM'}`);
  }
  return labels;
}

// For a future day there's no "past times to skip" concern the way there is for today, so
// this starts from a fixed clinic-opening time. Kept short (see FUTURE_DAY_CANDIDATE_COUNT)
// -- scanning every rejected candidate costs a real few seconds each (the alert-wait race in
// tryCandidateSlots), so trying many is what makes the scan itself risk taking longer than
// any reasonable test timeout. A brand new day is far less likely to already be picked over
// by this suite's own repeated runs than today is, so a short scan should suffice.
function fullDaySlotLabels(stepMinutes: number, count: number): string[] {
  const labels: string[] = [];
  for (let i = 0; i < count; i++) {
    const totalMinutes = 8 * 60 + i * stepMinutes;
    const hour24 = Math.floor(totalMinutes / 60);
    const minute = totalMinutes % 60;
    const hour12 = hour24 % 12 === 0 ? 12 : hour24 % 12;
    labels.push(`${hour12}:${String(minute).padStart(2, '0')} ${hour24 < 12 ? 'AM' : 'PM'}`);
  }
  return labels;
}

async function tryCandidateSlots(page: Page, dialog: Locator, candidateLabels: string[]): Promise<string | null> {
  const confirmationDialog = page.getByRole('dialog').filter({ hasText: 'Booking Confirmation' });
  const unavailableAlert = page.getByRole('alertdialog').filter({ hasText: /doctor is not available/i });

  for (const label of candidateLabels) {
    const slotButton = dialog.getByRole('button', { name: new RegExp(`^${label.replace(':', '\\:')}$`, 'i') }).first();
    if (!(await slotButton.scrollIntoViewIfNeeded().then(() => true).catch(() => false))) continue;
    if (!(await slotButton.count())) continue;
    await clickCenter(page, slotButton);

    const outcome = await Promise.race([
      confirmationDialog.waitFor({ state: 'visible', timeout: 3000 }).then(() => 'confirmed' as const),
      unavailableAlert.waitFor({ state: 'visible', timeout: 3000 }).then(() => 'unavailable' as const),
    ]).catch(() => 'timeout' as const);

    if (outcome === 'confirmed') return label;
    if (outcome === 'unavailable') {
      await clickCenter(page, unavailableAlert.getByRole('button', { name: /^ok$/i }).first());
    }
  }
  return null;
}

// Offline's grid only offers :00/:30 slots; Online's finer-grained grid also has :15/:45. A
// wide candidate range matters on this shared test DB: repeated runs against the same doctor
// (every flow in this suite uses the same one) permanently consume whatever slots they book,
// with no way to free them back up -- so today's calendar can end up with nothing bookable at
// all (confirmed live). If that happens, the dialog's own calendar grid lets any future date
// be picked, so advance a day at a time and try again there instead of giving up. Candidate
// counts are kept modest on purpose: each rejected candidate costs a real few seconds (the
// alert-wait race in tryCandidateSlots), so scanning too many risks the whole search taking
// longer than any reasonable test timeout even before it gets to a day that actually works.
const TODAY_CANDIDATE_COUNT = 10;
const FUTURE_DAY_CANDIDATE_COUNT = 10;
const MAX_DAYS_AHEAD = 2;

// The slot grid can still be empty/loading for a moment right after the dialog opens or right
// after a new day is picked (confirmed live: a scan immediately after either found zero
// time-shaped buttons at all) -- wait for at least one to actually render before scanning,
// rather than risk reading candidates as unavailable when the grid just hadn't loaded yet.
async function waitForSlotGridReady(dialog: Locator) {
  await expect(dialog.getByRole('button', { name: /^\d{1,2}:\d{2} (?:AM|PM)$/i }).first()).toBeVisible({
    timeout: 10000,
  });
}

export async function selectAvailableTimeSlot(page: Page, dialog: Locator, mode: AppointmentMode): Promise<string> {
  const stepMinutes = mode === 'Online' ? 15 : 30;

  await waitForSlotGridReady(dialog);
  const todaySlot = await tryCandidateSlots(page, dialog, computeCandidateSlotLabels(15, stepMinutes, TODAY_CANDIDATE_COUNT));
  if (todaySlot) return todaySlot;

  for (let daysAhead = 1; daysAhead <= MAX_DAYS_AHEAD; daysAhead++) {
    const date = new Date();
    date.setDate(date.getDate() + daysAhead);
    const dateLabel = date.toLocaleDateString('en-US', { weekday: 'long', month: 'long', day: 'numeric', year: 'numeric' });

    const dateButton = dialog.getByRole('button', { name: dateLabel }).first();
    const dateButtonReady = await dateButton.scrollIntoViewIfNeeded().then(() => true).catch(() => false);
    if (!dateButtonReady) continue;
    await clickCenter(page, dateButton);
    await waitForSlotGridReady(dialog);

    const futureSlot = await tryCandidateSlots(page, dialog, fullDaySlotLabels(stepMinutes, FUTURE_DAY_CANDIDATE_COUNT));
    if (futureSlot) return futureSlot;
  }

  throw new Error(`Could not find a bookable time slot today or over the next ${MAX_DAYS_AHEAD} days.`);
}

export async function bookAppointment(page: Page, patientName: string, mode: AppointmentMode) {
  const outpatientTab = page.getByRole('button', { name: /outpatient/i }).first();
  await expect(outpatientTab).toBeVisible({ timeout: 15000 });
  await clickCenter(page, outpatientTab);

  // The list renders as two separate <table> elements: a header-only table, and a
  // second table holding the actual data rows.
  const taskTable = page.getByRole('table').nth(1);
  await expect(taskTable).toBeVisible({ timeout: 15000 });
  const totalBefore = await readTaskListTotal(page);

  const searchBox = page.getByRole('textbox', { name: /enter the patient's name or phone number/i }).first();
  await expect(searchBox).toBeVisible({ timeout: 15000 });
  await typeInto(page, searchBox, patientName);

  const proceedButton = page.getByRole('button', { name: /^proceed$/i }).first();
  await expect(proceedButton).toBeVisible({ timeout: 15000 });
  await clickCenter(page, proceedButton);

  const bookingDialog = page.getByRole('dialog').filter({ hasText: 'Book Appointments' });
  await expect(bookingDialog).toBeVisible({ timeout: 15000 });

  // Appointment Mode defaults to Offline. Switching to Online swaps the slot grid to
  // 15-minute intervals across the full day (vs Offline's 30-minute clinic-hours slots) —
  // everything else in the dialog (Centre/Department/Doctor, the confirmation flow) is
  // identical, so this dropdown selection is the only mode-specific step.
  if (mode === 'Online') {
    await clickCenter(page, bookingDialog.getByRole('button', { name: /^offline$/i }).first());
    await clickCenter(page, page.getByRole('button', { name: /^online$/i }).first());
    // The "Online" label updates before the slot grid finishes reloading, so grabbing a
    // slot right away can still click the stale Offline grid mid-transition (confirmed
    // live: the booking silently went through as Offline). Wait for a :15/:45 slot — only
    // present once the finer-grained Online grid has actually loaded — before proceeding.
    await expect(bookingDialog.getByRole('button', { name: /:(?:15|45) (?:AM|PM)$/i }).first()).toBeVisible({
      timeout: 15000,
    });
  }

  // Confirm the dialog is actually configured for the intended mode right before booking.
  // This is a more reliable signal than trying to identify the new row afterward: the task
  // list sorts by appointment time-of-day (not creation order — confirmed live, an existing
  // later-in-the-day booking stayed "last" after adding an earlier one) and its own search
  // box only matches patient/doctor name, not date or time, so pinpointing the exact new
  // row post-hoc proved unreliable even after narrowing by patient.
  await expect(bookingDialog.getByRole('button', { name: new RegExp(`^${mode}$`, 'i') }).first()).toBeVisible();

  const slotLabel = await selectAvailableTimeSlot(page, bookingDialog, mode);

  const confirmationDialog = page.getByRole('dialog').filter({ hasText: 'Booking Confirmation' });
  await expect(confirmationDialog).toContainText(slotLabel);

  const confirmButton = page.getByRole('button', { name: /^confirm booking$/i }).first();
  await clickCenter(page, confirmButton);

  // The booking is processed asynchronously; the dialog stays open (its button replaced
  // by a spinner) until the appointment is actually created, then it closes on its own.
  await expect(confirmationDialog).toBeHidden({ timeout: 30000 });

  // Confirming can return to the Book Appointments dialog (so staff can add another slot)
  // instead of closing everything — dismiss it via its close button if still open.
  if (await bookingDialog.isVisible().catch(() => false)) {
    await clickCenter(page, bookingDialog.getByRole('button').first());
  }

  // Closing the dialog triggers a full re-render of the task list (it briefly disappears,
  // same loading state seen elsewhere in this app), so wait for it to come back first.
  await expect(taskTable).toBeVisible({ timeout: 15000 });
  await expect(async () => {
    expect(await readTaskListTotal(page)).toBeGreaterThan(totalBefore);
  }).toPass({ timeout: 15000 });
}
