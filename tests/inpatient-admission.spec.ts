// To run: RUN_NOSCURA_LOGIN_SMOKE=1 LOGIN_EMAIL=<email> LOGIN_PASSWORD=<password> npx playwright test tests/inpatient-admission.spec.ts
import { expect, test } from '@playwright/test';
import { loginEmail, loginPassword, runLoginSmoke } from './support/config';
import { login } from './support/auth';
import { admitPatient } from './support/inpatient';

test.describe('Noscura inpatient admission', () => {
  test.skip(
    !runLoginSmoke,
    'Set RUN_NOSCURA_LOGIN_SMOKE=1 to run the external Noscura admin login smoke test.'
  );
  test.skip(!loginEmail, 'Set LOGIN_EMAIL before running this test.');
  test.skip(!loginPassword, 'Set LOGIN_PASSWORD before running this test.');

  test('staff can create a patient and admit them as an inpatient', async ({ page }) => {
    test.setTimeout(150000);
    await login(page);

    const patientName = await admitPatient(page, 'Emergency Contact Person');

    // Submitting creates a new row at the top of the Inpatient dashboard's patient list.
    const patientRow = page.getByRole('row', { name: new RegExp(patientName, 'i') }).first();
    await expect(patientRow).toBeVisible({ timeout: 15000 });
  });
});
