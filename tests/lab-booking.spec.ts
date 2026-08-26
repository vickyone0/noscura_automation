// To run: RUN_NOSCURA_LOGIN_SMOKE=1 LOGIN_EMAIL=<email> LOGIN_PASSWORD=<password> npx playwright test tests/lab-booking.spec.ts
import { expect, test } from '@playwright/test';
import { loginEmail, loginPassword, runLoginSmoke } from './support/config';
import { login } from './support/auth';
import { bookLabTest } from './support/lab';

test.describe('Noscura lab booking', () => {
  test.skip(
    !runLoginSmoke,
    'Set RUN_NOSCURA_LOGIN_SMOKE=1 to run the external Noscura admin login smoke test.'
  );
  test.skip(!loginEmail, 'Set LOGIN_EMAIL before running this test.');
  test.skip(!loginPassword, 'Set LOGIN_PASSWORD before running this test.');

  test('staff can search a patient and book a lab test', async ({ page }) => {
    test.setTimeout(60000);
    await login(page);

    await bookLabTest(page, 'Aneesh', 'CBC', 'Kishan D');

    // Submitting creates a new row in the Laboratory dashboard's task list with Origin "Lab".
    const taskTable = page.getByRole('table').nth(1);
    await expect(taskTable).toBeVisible({ timeout: 15000 });
    await expect(taskTable.getByRole('row', { name: /Aneesh/i }).filter({ hasText: 'Lab' }).first()).toBeVisible({
      timeout: 15000,
    });
  });
});
