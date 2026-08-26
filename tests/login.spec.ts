// To run: RUN_NOSCURA_LOGIN_SMOKE=1 LOGIN_EMAIL=<email> LOGIN_PASSWORD=<password> npx playwright test tests/login.spec.ts
import { expect, test } from '@playwright/test';
import { loginEmail, loginPassword, runLoginSmoke } from './support/config';
import { clickCenter, typeInto } from './support/flutter';
import { openLoginForm, submitAndWaitForNavigation } from './support/auth';

test.describe('Noscura admin login', () => {
  test.skip(
    !runLoginSmoke,
    'Set RUN_NOSCURA_LOGIN_SMOKE=1 to run the external Noscura admin login smoke test.'
  );
  test.skip(!loginEmail, 'Set LOGIN_EMAIL before running this test.');
  test.skip(!loginPassword, 'Set LOGIN_PASSWORD before running this test.');

  test('user can log in with a valid email and password', async ({ page }) => {
    const { emailInput, passwordInput, submitButton } = await openLoginForm(page);

    await typeInto(page, emailInput, loginEmail!);
    await typeInto(page, passwordInput, loginPassword!);
    await submitAndWaitForNavigation(page, submitButton);

    await expect(page).not.toHaveURL(/\/login(?:$|[?#])/);
  });

  test('login is rejected for a badly formatted email', async ({ page }) => {
    const { emailInput, passwordInput, submitButton } = await openLoginForm(page);

    await typeInto(page, emailInput, 'not-an-email');
    await typeInto(page, passwordInput, loginPassword!);
    await clickCenter(page, submitButton);

    await expect(page.getByText(/badly formatted/i).first()).toBeVisible();
    await expect(page).toHaveURL(/\/login(?:$|[?#])/);
  });

  test('login is rejected for a well-formed but unregistered email', async ({ page }) => {
    const { emailInput, passwordInput, submitButton } = await openLoginForm(page);
    const unregisteredEmail = `does-not-exist-${Date.now()}@noscura.in`;

    await typeInto(page, emailInput, unregisteredEmail);
    await typeInto(page, passwordInput, loginPassword!);
    await clickCenter(page, submitButton);

    await expect(page.getByText(/incorrect|malformed|expired|not found|no user/i).first()).toBeVisible();
    await expect(page).toHaveURL(/\/login(?:$|[?#])/);
  });
});
