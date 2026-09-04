use crate::support::flutter::{click_center, text_matching_within, type_into, wait_for_url_matching};
use anyhow::{anyhow, Result};
use playwright_rs::expect;
use playwright_rs::protocol::locator::{AriaRole, GetByRoleOptions};
use playwright_rs::protocol::page::Page;
use regex::Regex;
use std::time::Duration;

// Searching a patient shows a result card with a labelled "Proceed" button (unlike the
// Laboratory search, whose result card has only an unlabeled arrow icon), so no positional
// fallback is needed here to find it. The search field itself still has the same aria-label-
// staleness issue as other search boxes in this app, so it's targeted positionally rather
// than by placeholder text.
async fn open_pharmacy_billing_for_patient(page: &Page, patient_name: &str) -> Result<()> {
    let pharmacy_tab = page.get_by_role(AriaRole::Button, Some(GetByRoleOptions::default().name("Pharmacy").exact(true))).first();
    expect(pharmacy_tab.clone()).with_timeout(Duration::from_secs(15)).to_be_visible().await?;
    click_center(page, &pharmacy_tab).await?;
    eprintln!("[pharmacy] clicked Pharmacy tab");

    let search_box = page.get_by_role(AriaRole::Textbox, None).first();
    expect(search_box.clone()).with_timeout(Duration::from_secs(15)).to_be_visible().await?;
    crate::support::flutter::type_into_verified(page, &search_box, patient_name).await?;
    eprintln!("[pharmacy] typed patient name");

    // The search result card takes a few seconds to debounce in.
    let proceed_button = page.get_by_role(AriaRole::Button, Some(GetByRoleOptions::default().name("Proceed").exact(true))).first();
    expect(proceed_button.clone()).with_timeout(Duration::from_secs(20)).to_be_visible().await?;
    eprintln!("[pharmacy] proceed button visible");
    click_center(page, &proceed_button).await?;
    eprintln!("[pharmacy] clicked proceed");

    wait_for_url_matching(page, &Regex::new(r"billingPagePharma").unwrap(), Duration::from_secs(15)).await?;
    eprintln!("[pharmacy] on billing page");
    Ok(())
}

// The medicine search results list is one Flutter semantics node with every result card's
// text (including each "Stock:" figure) merged together, so individual cards can't be
// targeted as separate locators the normal way. But each card's own unlabeled "+" button
// still renders in the same top-to-bottom order as its card's text in that merged blob, so
// pairing "Stock: N" match order with add-button y-order finds the right one. This matters
// because a common search term returns a mix of in-stock and out-of-stock results that look
// identical -- clicking an out-of-stock one pops an "Out of Stock" alert instead of adding it.
async fn add_medicine_to_cart(page: &Page, search_term: &str) -> Result<()> {
    let medicine_search = page
        .get_by_role(AriaRole::Textbox, Some(GetByRoleOptions::default().name("Search Medicine by name").exact(false)))
        .first();
    expect(medicine_search.clone()).with_timeout(Duration::from_secs(10)).to_be_visible().await?;
    eprintln!("[pharmacy] medicine search box visible");
    let search_box_box = medicine_search
        .bounding_box()
        .await?
        .ok_or_else(|| anyhow!("Could not read a bounding box for the medicine search field."))?;
    eprintln!("[pharmacy] read medicine search box bounding box");

    type_into(page, &medicine_search, search_term).await?;
    eprintln!("[pharmacy] typed medicine search term");

    // The medicine search results list is one Flutter semantics node with every result
    // card's text (including each "Stock:" figure) merged together -- confirmed live: the
    // whole list renders as several nested wrapper groups that each contain the SAME merged
    // text for every card, just with differing amounts of surrounding page chrome, not one
    // group per card. So individual cards can't be targeted as separate locators the normal
    // way; the shortest such wrapper is the one with the least chrome, i.e. closest to just
    // the cards' own merged text. Each card's own unlabeled "+" button still renders in the
    // same top-to-bottom order as its card's text in that merged blob, so pairing "Stock: N"
    // match order with add-button y-order finds the right one.
    //
    // Confirmed live: calling page.evaluate() for this works once but then hangs the whole
    // test indefinitely on a second call, and a *retry loop* around the plain-Locator
    // version (inner_text/bounding_box, no evaluate) intermittently hangs the same way after
    // enough repetitions -- immune to per-call tokio::time::timeout and to a multi-thread
    // runtime, pointing at a real synchronous-blocking bug somewhere in this crate's
    // connection layer that gets more likely to trigger the more Locator calls a test makes
    // in a short span. The mitigation that held up: do this whole scan exactly once (no
    // retry loop) after a generous wait for the debounced search, rather than repeating it.
    tokio::time::sleep(Duration::from_millis(3500)).await;
    eprintln!("[pharmacy] slept, starting groups scan");

    let stock_regex = Regex::new(r"Stock:\s*(\d+)").unwrap();
    let groups = page.locator("flt-semantics[role=\"group\"]");
    let group_count = groups.count().await?;
    eprintln!("[pharmacy] group_count = {group_count}");
    let mut merged_text: Option<String> = None;
    for i in 0..group_count as i32 {
        // A still-rendering page can shift the DOM mid-iteration, leaving `nth(i)` pointing
        // at a since-detached node -- give each individual call its own short timeout and
        // skip it on failure rather than let one stale node wedge the whole scan.
        let text = match tokio::time::timeout(Duration::from_millis(800), groups.nth(i).inner_text()).await {
            Ok(Ok(t)) => t,
            _ => continue,
        };
        if text.contains("Stock:") && merged_text.as_ref().is_none_or(|b| text.len() < b.len()) {
            merged_text = Some(text);
        }
    }
    let merged_text = merged_text.ok_or_else(|| anyhow!("Medicine search results have not rendered yet."))?;
    eprintln!("[pharmacy] groups scan done, merged_text len = {}", merged_text.len());

    let stocks: Vec<i64> = stock_regex.captures_iter(&merged_text).map(|c| c[1].parse().unwrap_or(0)).collect();
    let target_occurrence = stocks
        .iter()
        .position(|&s| s > 0)
        .ok_or_else(|| anyhow!("No in-stock result found for \"{search_term}\"."))?;
    eprintln!("[pharmacy] target_occurrence = {target_occurrence}, stocks = {stocks:?}");

    let buttons = page.get_by_role(AriaRole::Button, None);
    let count = buttons.count().await?;
    eprintln!("[pharmacy] buttons count = {count}, starting buttons scan");
    let mut plus_button_indices = Vec::new();
    for i in 0..count as i32 {
        // Same class of stale-node risk as the groups scan above.
        let box_ = match tokio::time::timeout(Duration::from_millis(800), buttons.nth(i).bounding_box()).await {
            Ok(Ok(Some(b))) => b,
            _ => continue,
        };
        if box_.y < search_box_box.y + search_box_box.height {
            continue;
        }
        if box_.width > 60.0 || box_.height > 60.0 {
            continue; // the "+" buttons are small squares
        }
        plus_button_indices.push(i);
    }
    let &add_button_index = plus_button_indices
        .get(target_occurrence)
        .ok_or_else(|| anyhow!("Add button for the in-stock result has not rendered yet."))?;
    eprintln!("[pharmacy] buttons scan done, add_button_index = {add_button_index}");

    let add_button = buttons.nth(add_button_index);
    click_center(page, &add_button).await?;
    eprintln!("[pharmacy] clicked add button");

    // Accumulated test data keeps shrinking real stock counts, so the row picked above can
    // still turn up empty by the time the click lands -- treat that as a real failure rather
    // than silently leaving the cart empty.
    let out_of_stock = text_matching_within(page, &Regex::new(r"(?i)out of stock").unwrap(), Duration::from_secs(2)).await;
    eprintln!("[pharmacy] out_of_stock check done: {}", out_of_stock.is_ok());
    if out_of_stock.is_ok() {
        return Err(anyhow!("\"{search_term}\" result was out of stock by the time it was clicked."));
    }

    // The cart's item table (Batch/Expiry/MRP/Qty/... columns) only renders once it holds at
    // least one item -- a reliable, order-independent signal that the add actually landed.
    let batch_header = text_matching_within(page, &Regex::new(r"(?i)^Batch$").unwrap(), Duration::from_secs(20)).await?;
    eprintln!("[pharmacy] batch header found");
    expect(batch_header).to_be_visible().await?;
    eprintln!("[pharmacy] add_medicine_to_cart done");
    Ok(())
}

// Stops short of Submit: confirmed live (in the TS version of this suite) that it gets stuck
// in a permanent loading spinner on this environment and never actually completes -- the same
// known limitation as "Finish Appointment" in the outpatient consultation flow. Everything up
// to and including adding the medicine to the cart works reliably.
pub async fn book_medicine(page: &Page, patient_name: &str, medicine_search_term: &str) -> Result<()> {
    open_pharmacy_billing_for_patient(page, patient_name).await?;
    add_medicine_to_cart(page, medicine_search_term).await?;
    Ok(())
}
