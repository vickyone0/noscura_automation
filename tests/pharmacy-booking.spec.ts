// To run: RUN_NOSCURA_LOGIN_SMOKE=1 LOGIN_EMAIL=<email> LOGIN_PASSWORD=<password> npx playwright test tests/pharmacy-booking.spec.ts
import { expect, test } from '@playwright/test';
import { loginEmail, loginPassword, runLoginSmoke } from './support/config';
import { login } from './support/auth';
import { bookMedicine } from './support/pharmacy';

test.describe('Noscura pharmacy medicine booking', () => {
  test.skip(
    !runLoginSmoke,
    'Set RUN_NOSCURA_LOGIN_SMOKE=1 to run the external Noscura admin login smoke test.'
  );
  test.skip(!loginEmail, 'Set LOGIN_EMAIL before running this test.');
  test.skip(!loginPassword, 'Set LOGIN_PASSWORD before running this test.');

  test('staff can search a patient and add a medicine to their pharmacy cart', async ({ page }) => {
    test.setTimeout(60000);
    await login(page);

    await bookMedicine(page, 'Aneesh', 'Dolo');
  });
});
