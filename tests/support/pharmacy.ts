import { expect, type Page } from '@playwright/test';
import { clickCenter, typeInto, typeIntoVerified } from './flutter';

// Searching a patient shows a result card with a labelled "Proceed" button (unlike the
// Laboratory search, whose result card has only an unlabeled arrow icon), so no positional
// fallback is needed here to find it. The search field itself still has the same aria-label-
// staleness issue as other search boxes in this app (its accessible name becomes whatever was
// typed once it has a value), so it's targeted positionally rather than by placeholder text.
async function openPharmacyBillingForPatient(page: Page, patientName: string) {
  const pharmacyTab = page.getByRole('button', { name: /^pharmacy$/i }).first();
  await expect(pharmacyTab).toBeVisible({ timeout: 15000 });
  await clickCenter(page, pharmacyTab);

  const searchBox = page.getByRole('textbox').first();
  await expect(searchBox).toBeVisible({ timeout: 15000 });
  await typeIntoVerified(page, searchBox, patientName);

  // The search result card takes a few seconds to debounce in; `expect` polls until it does.
  const proceedButton = page.getByRole('button', { name: /^proceed$/i }).first();
  await expect(proceedButton).toBeVisible({ timeout: 20000 });
  await clickCenter(page, proceedButton);

  await page.waitForURL(/billingPagePharma/, { timeout: 15000 });
}

// The medicine search results list is one Flutter semantics node with every result card's
// text (including each "Stock:" figure) merged together, so individual cards can't be
// targeted as separate locators the normal way. But each card's own unlabeled "+" button
// still renders in the same top-to-bottom order as its card's text in that merged blob, so
// pairing "Stock: N" match order with add-button y-order finds the right one. This matters
// because a common search term returns a mix of in-stock and out-of-stock results that look
// identical — clicking an out-of-stock one pops an "Out of Stock" alert instead of adding it.
async function addMedicineToCart(page: Page, searchTerm: string) {
  const medicineSearch = page.getByRole('textbox', { name: /search medicine by name/i }).first();
  await expect(medicineSearch).toBeVisible({ timeout: 10000 });
  const searchBoxBox = await medicineSearch.boundingBox();
  if (!searchBoxBox) throw new Error('Could not read a bounding box for the medicine search field.');

  await typeInto(page, medicineSearch, searchTerm);

  let addButtonIndex = -1;
  await expect(async () => {
    const mergedText = await page.evaluate(() => {
      const nodes = Array.from(document.querySelectorAll('flt-semantics[role="group"]'));
      let best: string | null = null;
      for (const node of nodes) {
        const text = node.textContent ?? '';
        if (text.includes('Stock:') && (best === null || text.length < best.length)) best = text;
      }
      return best;
    });
    if (!mergedText) throw new Error('Medicine search results have not rendered yet.');

    const stocks = [...mergedText.matchAll(/Stock:\s*(\d+)/g)].map((match) => Number(match[1]));
    const targetOccurrence = stocks.findIndex((stock) => stock > 0);
    if (targetOccurrence === -1) throw new Error(`No in-stock result found for "${searchTerm}".`);

    const buttons = page.getByRole('button');
    const count = await buttons.count();
    const plusButtonIndices: number[] = [];
    for (let i = 0; i < count; i++) {
      const box = await buttons.nth(i).boundingBox();
      if (!box) continue;
      if (box.y < searchBoxBox.y + searchBoxBox.height) continue;
      if (box.width > 60 || box.height > 60) continue; // the "+" buttons are small squares
      plusButtonIndices.push(i);
    }
    if (plusButtonIndices.length <= targetOccurrence) {
      throw new Error('Add button for the in-stock result has not rendered yet.');
    }
    addButtonIndex = plusButtonIndices[targetOccurrence];
  }).toPass({ timeout: 10000 });

  await clickCenter(page, page.getByRole('button').nth(addButtonIndex));

  // Accumulated test data keeps shrinking real stock counts, so the row picked above can
  // still turn up empty by the time the click lands — treat that as a real failure rather
  // than silently leaving the cart empty.
  const outOfStockAlert = await expect(page.getByText(/out of stock/i).first())
    .toBeVisible({ timeout: 2000 })
    .then(() => true)
    .catch(() => false);
  if (outOfStockAlert) throw new Error(`"${searchTerm}" result was out of stock by the time it was clicked.`);

  // The cart's item table (Batch/Expiry/MRP/Qty/... columns) only renders once it holds at
  // least one item — a reliable, order-independent signal that the add actually landed.
  await expect(page.getByText(/^Batch$/i).first()).toBeVisible({ timeout: 10000 });
}

// Stops short of Submit: confirmed live that it gets stuck in a permanent loading spinner on
// this environment and never actually completes (the page never navigates away) — the same
// known limitation as "Finish Appointment" in the outpatient consultation flow. Everything up
// to and including adding the medicine to the cart works reliably.
export async function bookMedicine(page: Page, patientName: string, medicineSearchTerm: string) {
  await openPharmacyBillingForPatient(page, patientName);
  await addMedicineToCart(page, medicineSearchTerm);
}
