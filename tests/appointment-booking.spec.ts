// To run: RUN_NOSCURA_LOGIN_SMOKE=1 LOGIN_EMAIL=<email> LOGIN_PASSWORD=<password> npx playwright test tests/appointment-booking.spec.ts
import { test } from '@playwright/test';
import { loginEmail, loginPassword, runLoginSmoke } from './support/config';
import { login } from './support/auth';
import { bookAppointment } from './support/appointments';
import { createPatient } from './support/new-patient';

test.describe('Noscura patient search and appointment booking', () => {
  test.skip(
    !runLoginSmoke,
    'Set RUN_NOSCURA_LOGIN_SMOKE=1 to run the external Noscura admin login smoke test.'
  );
  test.skip(!loginEmail, 'Set LOGIN_EMAIL before running this test.');
  test.skip(!loginPassword, 'Set LOGIN_PASSWORD before running this test.');

  // Each test books against a freshly created patient rather than a fixed name: a shared,
  // reused patient's state (an existing task, an admission from another suite) can change
  // what appears after searching for them (confirmed live) and break the flow this test
  // means to exercise.
  test('staff can search a patient and book an offline appointment', async ({ page }) => {
    test.setTimeout(200000);
    await login(page);

    const patientName = await createPatient(page, 'Appointment');
    await bookAppointment(page, patientName, 'Offline');
  });

  test('staff can search a patient and book an online appointment', async ({ page }) => {
    // More candidate slots may need trying here than for Offline (see selectAvailableTimeSlot).
    test.setTimeout(300000);
    await login(page);

    const patientName = await createPatient(page, 'Appointment');
    await bookAppointment(page, patientName, 'Online');
  });
});
