// To run: RUN_NOSCURA_LOGIN_SMOKE=1 LOGIN_EMAIL=<email> LOGIN_PASSWORD=<password> npx playwright test tests/consultation.spec.ts
import { expect, test } from '@playwright/test';
import { loginEmail, loginPassword, runLoginSmoke } from './support/config';
import { login } from './support/auth';
import { bookAppointment } from './support/appointments';
import { addImagingTest, addMedicine, enterAppointment } from './support/consultation';
import { createPatient } from './support/new-patient';

test.describe('Noscura outpatient consultation', () => {
  test.skip(
    !runLoginSmoke,
    'Set RUN_NOSCURA_LOGIN_SMOKE=1 to run the external Noscura admin login smoke test.'
  );
  test.skip(!loginEmail, 'Set LOGIN_EMAIL before running this test.');
  test.skip(!loginPassword, 'Set LOGIN_PASSWORD before running this test.');

  // enterAppointment finds its patient by row in the outpatient task list, so this needs a
  // patient with an existing booked appointment -- create one fresh and book it here rather
  // than relying on a fixed patient name already having one (confirmed live: a shared, reused
  // patient's task-list state from another suite's run can leave no bookable/enterable
  // appointment, or one already mid-consultation, breaking this flow in ways unrelated to
  // what this test means to check).
  //
  // Stops short of "Finish Appointment": confirmed live that it gets stuck in a permanent
  // loading spinner on this environment and never actually completes (reloading the
  // appointment resets it back to the pre-finish state), independent of anything this test
  // does. Everything up to and including saving the medicine/imaging orders works reliably.
  test('staff can add a medicine and an imaging test during a consultation', async ({ page }) => {
    test.setTimeout(300000);
    await login(page);

    const patientName = await createPatient(page, 'Consultation');
    await bookAppointment(page, patientName, 'Offline');

    await enterAppointment(page, patientName);

    await addMedicine(page, 'Dolo');
    await expect(page.getByText(/^Dolo/i).first()).toBeVisible();

    await addImagingTest(page, 'X-Ray', 'Chest');
    await expect(page.getByText(/^X-Ray$/i).first()).toBeVisible();
  });
});
