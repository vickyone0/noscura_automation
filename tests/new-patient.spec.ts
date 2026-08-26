// To run: RUN_NOSCURA_LOGIN_SMOKE=1 LOGIN_EMAIL=<email> LOGIN_PASSWORD=<password> npx playwright test tests/new-patient.spec.ts
import { expect, test } from '@playwright/test';
import { loginEmail, loginPassword, runLoginSmoke } from './support/config';
import { clickCenter } from './support/flutter';
import { login } from './support/auth';
import { expectNewPatientFormVisible, fillRequiredNewPatientFields, gotoNewPatientForm } from './support/new-patient';

test.describe('Noscura new patient page', () => {
  test.describe.configure({ mode: 'serial' });

  test.skip(
    !runLoginSmoke,
    'Set RUN_NOSCURA_LOGIN_SMOKE=1 to run the external Noscura admin login smoke test.'
  );
  test.skip(!loginEmail, 'Set LOGIN_EMAIL before running this test.');
  test.skip(!loginPassword, 'Set LOGIN_PASSWORD before running this test.');

  test('staff can open the new patient page and start patient creation', async ({ page }) => {
    test.setTimeout(60000);
    await login(page);

    await gotoNewPatientForm(page);
    await expectNewPatientFormVisible(page);
  });

  test('new patient creation shows required-field validation for a blank submit', async ({ page }) => {
    test.setTimeout(60000);
    await login(page);
    await gotoNewPatientForm(page);

    await clickCenter(page, page.getByRole('button', { name: /^submit$/i }).first());

    await expect(page).toHaveURL(/\/addPatient(?:$|[?#])/);
    await expect(page.getByText(/^Create New Patient$/i).first()).toBeVisible();
    await expect(page.getByText(/Field is required/i).first()).toBeVisible();
  });

  test('staff can enter new patient details and cancel without creating a record', async ({ page }) => {
    test.setTimeout(60000);
    await login(page);
    await gotoNewPatientForm(page);

    const suffix = String(Date.now()).slice(-6);
    await fillRequiredNewPatientFields(page, {
      name: `Auto Patient ${suffix}`,
      patientId: `AUTO${suffix}`,
      phoneNumber: `9000${suffix}`,
    });
    await clickCenter(page, page.getByRole('button', { name: /^cancel$/i }).first());

    await expect(page).toHaveURL(/\/newHomeAdmin\?selectedOption=1(?:$|[&#])/);
    await expect(page.getByText(/Patients List/i).first()).toBeVisible({ timeout: 15000 });
    await expect(page.getByRole('button', { name: /^new patient$/i }).first()).toBeVisible();
  });

  [
    {
      title: 'accepts a female patient with a numeric patient id and young age',
      gender: 'Female' as const,
      age: '1',
      patientIdPrefix: '12345',
      namePrefix: 'Auto Numeric',
      address: 'Flat 12, 4th Main Road, Bengaluru',
    },
    {
      title: 'accepts an other-gender patient with an alphanumeric patient id and older age',
      gender: 'Other' as const,
      age: '99',
      patientIdPrefix: 'NC-AUTO',
      namePrefix: 'Auto Mixed',
      address: 'Automation test address with landmark near main reception',
    },
  ].forEach(({ title, gender, age, patientIdPrefix, namePrefix, address }) => {
    test(`new patient form ${title}`, async ({ page }) => {
      test.setTimeout(60000);
      await login(page);
      await gotoNewPatientForm(page);

      const suffix = String(Date.now()).slice(-6);
      await fillRequiredNewPatientFields(page, {
        name: `${namePrefix} ${suffix}`,
        patientId: `${patientIdPrefix}${suffix}`,
        phoneNumber: `9100${suffix}`,
        address,
        age,
        gender,
      });
      await clickCenter(page, page.getByRole('button', { name: /^cancel$/i }).first());

      await expect(page).toHaveURL(/\/newHomeAdmin\?selectedOption=1(?:$|[&#])/);
      await expect(page.getByText(/Patients List/i).first()).toBeVisible({ timeout: 15000 });
    });
  });
});
