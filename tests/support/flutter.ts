import { expect, type Locator, type Page } from '@playwright/test';

// The admin app is a Flutter web build (CanvasKit renderer): the UI is painted onto a
// <canvas>, so form fields don't exist as real DOM nodes until Flutter's accessibility/
// semantics tree is turned on. The "Enable accessibility" node is Flutter's own a11y
// toggle (invisible, 1x1px, off-screen) — not a bot/captcha challenge — so we trigger it
// to get located, then interact via real mouse/keyboard events, which is what Flutter's
// canvas actually listens to (synthetic DOM clicks/fills on semantics nodes are ignored).
// Flutter's canvas layout keeps settling/animating for a bit after each render, so the
// element's position can still be drifting when we read it. Poll until two consecutive
// reads agree before clicking, so we don't click where the element used to be.
export async function boundingBoxWhenStable(
  locator: Locator
): Promise<{ x: number; y: number; width: number; height: number }> {
  await expect(locator).toBeVisible({ timeout: 5000 });
  // A "visible" element can still sit below the fold on a page taller than the viewport (e.g.
  // the lab booking form's Submit button) — its bounding box is then partly or fully outside
  // the viewport, so a mouse click at its center coordinates lands nowhere and is silently a
  // no-op. Scroll it into view before reading/clicking its position.
  await locator.scrollIntoViewIfNeeded();
  let previous = await locator.boundingBox({ timeout: 5000 });
  for (let i = 0; i < 20; i++) {
    await new Promise((resolve) => setTimeout(resolve, 50));
    const current = await locator.boundingBox();
    if (
      previous &&
      current &&
      previous.x === current.x &&
      previous.y === current.y &&
      previous.width === current.width &&
      previous.height === current.height
    ) {
      return current;
    }
    previous = current;
  }
  if (!previous) throw new Error('Element has no bounding box to click.');
  return previous;
}

export async function clickCenter(page: Page, locator: Locator) {
  const box = await boundingBoxWhenStable(locator);
  await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
}

// A click occasionally lands a frame before the underlying input is ready to take focus
// (same class of race as the login submit button), so retry the click rather than
// failing the whole test on one missed frame. As a last resort, request focus directly via
// the DOM API — bypasses pointer-coordinate ambiguity entirely — for the rare field where a
// real click still doesn't land it (confirmed live: reproduces only under the Playwright test
// runner, not in a plain script against the same page, so the exact cause is unclear).
export async function typeInto(page: Page, locator: Locator, text: string) {
  for (let attempt = 1; attempt <= 3; attempt++) {
    await clickCenter(page, locator);
    try {
      await expect(locator).toBeFocused({ timeout: 3000 });
      await page.keyboard.type(text, { delay: 20 });
      return;
    } catch (error) {
      if (attempt < 3) continue;
      await locator.evaluate((el) => (el as HTMLElement).focus());
      if (!(await locator.evaluate((el) => el === document.activeElement))) throw error;
      await page.keyboard.type(text, { delay: 20 });
    }
  }
}

// Even once a field is confirmed focused, its very first keystroke can still be dropped if
// Flutter's IME isn't quite ready the instant DOM focus lands (confirmed live: "Aneesh"
// landed as "neesh" despite `typeInto`'s own focus check passing first). Verify what actually
// landed and, if it's wrong, clear it with real keyboard events — Playwright's `.fill()` uses
// native interaction rather than a real click, which doesn't reliably work against this app's
// canvas (the whole reason `clickCenter`/`typeInto` exist) — and retype. Use this in place of
// `typeInto` only for fields worth this extra cost; most fields' occasional dropped-first-
// character risk isn't worth every call paying for a verification round trip.
export async function typeIntoVerified(page: Page, locator: Locator, text: string) {
  for (let attempt = 1; attempt <= 3; attempt++) {
    if (attempt > 1) {
      await clickCenter(page, locator);
      await page.keyboard.press('Control+A');
      await page.keyboard.press('Backspace');
    }
    await typeInto(page, locator, text);
    if ((await locator.inputValue().catch(() => null)) === text) return;
  }
}

export async function enableFlutterAccessibility(page: Page) {
  const a11yToggle = page.getByRole('button', { name: /enable accessibility/i });
  if (await a11yToggle.isVisible({ timeout: 15000 }).catch(() => false)) {
    await a11yToggle.dispatchEvent('click');
  }
}

// This form's fields don't render in visual reading order in Flutter's semantics tree (the
// Age field can appear before Address there), so a positional index into all textboxes picks
// the wrong one — confirmed live: nth(6) resolved to the Age input, not Address. The form also
// has no aria-labelledby linking a label to its input, so locate each field by whichever
// textbox sits just below/beside its visible label text instead, matching how a sighted user
// would find it.
export async function textboxNearLabel(page: Page, labelPattern: RegExp): Promise<Locator> {
  const label = page.getByText(labelPattern).first();
  const labelBox = await label.boundingBox();
  if (!labelBox) throw new Error(`Could not find a label matching ${labelPattern}.`);

  const textboxes = page.getByRole('textbox');
  const count = await textboxes.count();
  let best: { index: number; distance: number } | null = null;
  for (let i = 0; i < count; i++) {
    const box = await textboxes.nth(i).boundingBox();
    if (!box) continue;
    const verticalGap = box.y - labelBox.y;
    if (verticalGap < 0 || verticalGap > 120) continue; // must sit just below the label
    const horizontalGap = Math.max(0, labelBox.x - (box.x + box.width), box.x - (labelBox.x + labelBox.width));
    const distance = verticalGap + horizontalGap;
    if (!best || distance < best.distance) best = { index: i, distance };
  }
  if (best === null) throw new Error(`Could not find an input near a label matching ${labelPattern}.`);
  return textboxes.nth(best.index);
}

// `getByText(pattern).first()` picks whichever DOM-order match comes first — fine when a
// pattern is unique on the page, wrong once it isn't (confirmed live: a patient's name
// matched both a search-result card and their own rows further down an accumulated task
// list, and DOM order put a task-list row first, not the search card). Same "nearest below
// an anchor" approach as textboxNearLabel, generalized to any text pattern.
export async function textBelow(page: Page, anchor: Locator, pattern: RegExp, maxGap = 150): Promise<Locator> {
  const anchorBox = await anchor.boundingBox();
  if (!anchorBox) throw new Error('Could not read a bounding box for the anchor element.');

  const candidates = page.getByText(pattern);
  const count = await candidates.count();
  let best: { index: number; gap: number } | null = null;
  for (let i = 0; i < count; i++) {
    const box = await candidates.nth(i).boundingBox();
    if (!box) continue;
    const verticalGap = box.y - anchorBox.y;
    if (verticalGap < 0 || verticalGap > maxGap) continue;
    if (!best || verticalGap < best.gap) best = { index: i, gap: verticalGap };
  }
  if (best === null) throw new Error(`Could not find text matching ${pattern} below the anchor element.`);
  return candidates.nth(best.index);
}

type Box = { x: number; y: number; width: number; height: number };

// Several icon-only action buttons in this app (a search result's arrow, a "+" add-row
// button) have no accessible name at all, so they can't be targeted by role+name. They do
// sit reliably on the same row as some labelled anchor (a name, an input), just to its
// right — the same "find the nearest element instead of an exact selector" approach as
// textboxNearLabel above, generalized to buttons and any anchor locator. A plain box can be
// passed instead of a Locator for an anchor whose own role/name is about to change (e.g. an
// input that turns into a button-like display once a value is selected for it) — capture its
// position before that happens rather than re-resolving a locator that will no longer match.
export async function buttonRightOf(page: Page, anchor: Locator | Box): Promise<Locator> {
  const anchorBox = 'boundingBox' in anchor ? await anchor.boundingBox() : anchor;
  if (!anchorBox) throw new Error('Could not read a bounding box for the anchor element.');

  const buttons = page.getByRole('button');
  const count = await buttons.count();
  let best: { index: number; distance: number } | null = null;
  for (let i = 0; i < count; i++) {
    const box = await buttons.nth(i).boundingBox();
    if (!box) continue;
    const verticalGap = Math.abs(box.y - anchorBox.y);
    if (verticalGap > 40) continue; // same row only
    if (box.x < anchorBox.x + anchorBox.width) continue; // must sit to the right
    const distance = verticalGap + (box.x - (anchorBox.x + anchorBox.width));
    if (!best || distance < best.distance) best = { index: i, distance };
  }
  if (best === null) throw new Error('Could not find a button to the right of the anchor element.');
  return buttons.nth(best.index);
}

// This app's custom Flutter dropdowns ("Please select...") don't use native <select>, aria
// combobox roles, or aria-labelledby linking a label to its control, so — like textboxNearLabel
// — the closest "Please select..." text below a given label is the most reliable way to find
// one on a long form where several dropdowns share the same placeholder text. Anchoring on the
// label (re-read fresh each call) survives the form reflowing as earlier fields get filled in,
// unlike a positional index into all dropdowns, which breaks the moment an earlier field's
// validation message appears/disappears and shifts everything below it.
export async function dropdownNearLabel(page: Page, labelPattern: RegExp): Promise<Locator> {
  const label = page.getByText(labelPattern).first();
  await label.scrollIntoViewIfNeeded();

  // A single-shot position read here occasionally comes up empty under the Playwright test
  // runner even though the label and its dropdown are both already on screen (confirmed live:
  // the same lookup logic against the same page state succeeds reliably in a plain script) --
  // the same unexplained runner-only gap seen elsewhere in this suite. Retry the whole
  // read-and-match rather than failing on one bad frame.
  let match!: Locator;
  await expect(async () => {
    const labelBox = await label.boundingBox();
    if (!labelBox) throw new Error(`Could not find a label matching ${labelPattern}.`);

    const dropdowns = page.getByText(/^Please select\.\.\.$/i);
    const count = await dropdowns.count();
    let best: { index: number; distance: number } | null = null;
    for (let i = 0; i < count; i++) {
      const box = await dropdowns.nth(i).boundingBox();
      if (!box) continue;
      const verticalGap = box.y - labelBox.y;
      if (verticalGap < 0 || verticalGap > 100) continue; // must sit just below the label
      const horizontalGap = Math.max(0, labelBox.x - (box.x + box.width), box.x - (labelBox.x + labelBox.width));
      if (horizontalGap > 300) continue; // must sit in roughly the same column
      const distance = verticalGap + horizontalGap;
      if (!best || distance < best.distance) best = { index: i, distance };
    }
    if (best === null) throw new Error(`Could not find a dropdown near a label matching ${labelPattern}.`);
    match = dropdowns.nth(best.index);
  }).toPass({ timeout: 10000 });

  return match;
}

// Opening one of this app's dropdowns replaces the underlying form's semantics tree with the
// overlay's while it's open (confirmed live: the page's flt-semantics node count drops from
// dozens to a handful), and the option buttons' visible text isn't real DOM text content (their
// accessible name lives only in aria-label) — so options must be matched by accessible role and
// name, never by getByText or textContent. Opening the overlay can also silently fail to render
// on the first click (confirmed live via screenshots showing it still closed after one click,
// then open after a wait) with no visible difference to distinguish a genuine miss from normal
// render latency, so this waits generously for the option rather than retrying the click itself
// -- retrying the click risks toggling an already-open overlay shut again.
export async function selectDropdownOption(page: Page, dropdownLocator: Locator, optionPattern: RegExp) {
  const option = page.getByRole('button', { name: optionPattern }).first();
  await clickCenter(page, dropdownLocator);
  await expect(option).toBeVisible({ timeout: 8000 });
  await clickCenter(page, option);
}

// Same overlay mechanics as selectDropdownOption, for dropdowns (like Floor/Block, which
// cascades from Room Type) where any one option is fine and the exact label isn't worth
// hardcoding -- picks whichever option renders closest below/right of the dropdown itself.
export async function selectFirstDropdownOption(page: Page, dropdownLocator: Locator) {
  const dropdownBox = await dropdownLocator.boundingBox();
  if (!dropdownBox) throw new Error('Could not read a bounding box for the dropdown element.');

  await clickCenter(page, dropdownLocator);

  let optionIndex = -1;
  await expect(async () => {
    const buttons = page.getByRole('button');
    const count = await buttons.count();
    let best: { index: number; y: number } | null = null;
    for (let i = 0; i < count; i++) {
      const box = await buttons.nth(i).boundingBox();
      if (!box) continue;
      if (box.x < dropdownBox.x - 10 || box.x > dropdownBox.x + dropdownBox.width + 10) continue;
      if (box.y < dropdownBox.y || box.y > dropdownBox.y + 300) continue;
      if (!best || box.y < best.y) best = { index: i, y: box.y };
    }
    if (best === null) throw new Error('Dropdown options have not rendered yet.');
    optionIndex = best.index;
  }).toPass({ timeout: 8000 });

  await clickCenter(page, page.getByRole('button').nth(optionIndex));
}
