import { expect, type Locator, type Page } from '@playwright/test';
import { loginEmail, loginPassword, loginURL } from './config';
import { clickCenter, enableFlutterAccessibility, typeInto } from './flutter';

type LoginForm = {
  emailInput: Locator;
  passwordInput: Locator;
  submitButton: Locator;
};

export async function openLoginForm(page: Page): Promise<LoginForm> {
  await page.goto(loginURL);

  await enableFlutterAccessibility(page);

  const emailInput = page.getByRole('textbox', { name: /enter your email/i }).first();
  const passwordInput = page.locator('input[type="password"]').first();
  const submitButton = page.getByRole('button', { name: /^log in$/i }).first();

  await expect(emailInput).toBeVisible({ timeout: 15000 });
  await expect(passwordInput).toBeVisible({ timeout: 15000 });

  return { emailInput, passwordInput, submitButton };
}

// Against the live site, a submit click occasionally lands a frame before the button is
// actually interactive and the tap is dropped. Retry the click rather than the whole test.
export async function submitAndWaitForNavigation(page: Page, submitButton: Locator) {
  const stillOnLogin = () => /\/login(?:$|[?#])/.test(new URL(page.url()).pathname);
  for (let attempt = 1; attempt <= 3; attempt++) {
    if (!stillOnLogin()) return;
    try {
      await clickCenter(page, submitButton);
    } catch (error) {
      if (!stillOnLogin()) return;
      throw error;
    }
    try {
      await page.waitForURL((url) => !/\/login(?:$|[?#])/.test(url.pathname), { timeout: 5000 });
      return;
    } catch {
      if (attempt === 3 || !stillOnLogin()) throw new Error('Did not navigate away from /login after submitting.');
    }
  }
}

export async function login(page: Page) {
  const { emailInput, passwordInput, submitButton } = await openLoginForm(page);
  await typeInto(page, emailInput, loginEmail!);
  await typeInto(page, passwordInput, loginPassword!);
  await submitAndWaitForNavigation(page, submitButton);
}
