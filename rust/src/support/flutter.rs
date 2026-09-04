use anyhow::{anyhow, Result};
use playwright_rs::protocol::locator::{AriaRole, BoundingBox, GetByRoleOptions, Locator};
use playwright_rs::protocol::page::Page;
use playwright_rs::expect;
use regex::Regex;
use std::time::Duration;

// The admin app is a Flutter web build (CanvasKit renderer): the UI is painted onto a
// <canvas>, so form fields don't exist as real DOM nodes until Flutter's accessibility/
// semantics tree is turned on. The "Enable accessibility" node is Flutter's own a11y
// toggle (invisible, 1x1px, off-screen) -- not a bot/captcha challenge -- so we trigger it
// to get located, then interact via real mouse/keyboard events, which is what Flutter's
// canvas actually listens to (synthetic DOM clicks/fills on semantics nodes are ignored).
// Flutter's canvas layout keeps settling/animating for a bit after each render, so the
// element's position can still be drifting when we read it. Poll until two consecutive
// reads agree before clicking, so we don't click where the element used to be.
pub async fn bounding_box_when_stable(locator: &Locator) -> Result<BoundingBox> {
    // Right after a tab switch or other DOM transition, a broad locator (e.g. "first textbox
    // on the page") can momentarily resolve to a transient node that's already detaching by
    // the time the very next action runs against it -- `to_be_visible()` passes, then
    // `scroll_into_view_if_needed()` throws "Element is not attached to the DOM". Retry the
    // whole visible+scroll sequence a few times (re-resolving the locator each time) rather
    // than treating one such race as a hard failure.
    let mut last_err = None;
    for attempt in 0..4 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(300)).await;
        }
        // Confirmed live (comparing against a manual Playwright session hitting the exact same
        // element instantly): this crate's is_visible()/to_be_visible() sends an "isVisible"
        // protocol call carrying a 30-SECOND server-side auto-wait (crate::DEFAULT_TIMEOUT_MS),
        // unlike real Playwright's instant, non-waiting isVisible -- a single call can silently
        // cost up to 30 real seconds whenever the element isn't resolving instantaneously, which
        // made even a handful of retries here too slow to matter. bounding_box() is built on a
        // plain, timeout-free querySelector (confirmed in the crate source) and returns
        // immediately whether or not the element exists, so poll for a real, nonzero box
        // directly instead of gating on the expensive to_be_visible() check.
        let mut appeared = false;
        let poll_deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        while tokio::time::Instant::now() < poll_deadline {
            match tokio::time::timeout(Duration::from_secs(5), locator.bounding_box()).await {
                Ok(Ok(Some(bbox))) if bbox.width > 0.0 && bbox.height > 0.0 => {
                    appeared = true;
                    break;
                }
                _ => tokio::time::sleep(Duration::from_millis(100)).await,
            }
        }
        if !appeared {
            last_err = Some(anyhow!("Element never had a nonzero bounding box (not visible)."));
            continue;
        }
        // A "visible" element can still sit below the fold on a page taller than the viewport --
        // its bounding box is then partly or fully outside the viewport, so a mouse click at its
        // center coordinates lands nowhere and is silently a no-op. Scroll it into view first.
        // This call can also, confirmed live, hang indefinitely rather than error -- a plain
        // Err retry never fires on a genuine stall, so wrap it in its own timeout too.
        match tokio::time::timeout(Duration::from_secs(5), locator.scroll_into_view_if_needed()).await {
            Ok(Ok(())) => {
                last_err = None;
                break;
            }
            Ok(Err(e)) => last_err = Some(e.into()),
            Err(_) => last_err = Some(anyhow!("Timed out scrolling the element into view.")),
        }
    }
    if let Some(e) = last_err {
        return Err(e);
    }

    let mut previous = tokio::time::timeout(Duration::from_secs(5), locator.bounding_box())
        .await
        .map_err(|_| anyhow!("Timed out reading the element's bounding box."))??;
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let current = tokio::time::timeout(Duration::from_secs(5), locator.bounding_box())
            .await
            .map_err(|_| anyhow!("Timed out reading the element's bounding box."))??;
        if let (Some(p), Some(c)) = (&previous, &current) {
            if p.x == c.x && p.y == c.y && p.width == c.width && p.height == c.height {
                return Ok(c.clone());
            }
        }
        previous = current;
    }
    previous.ok_or_else(|| anyhow!("Element has no bounding box to click."))
}

pub async fn click_center(page: &Page, locator: &Locator) -> Result<()> {
    let bbox = bounding_box_when_stable(locator).await?;
    page.mouse()
        .click(bbox.x + bbox.width / 2.0, bbox.y + bbox.height / 2.0, None)
        .await?;
    Ok(())
}

// `to_be_focused()` doesn't reliably detect focus on this app's Flutter semantics elements
// even when a click has visibly focused the field (confirmed live via screenshot: a real
// blue focus outline was showing while the check still failed) -- so just click then type
// directly rather than gating on a check that doesn't match this app's reality.
pub async fn type_into(page: &Page, locator: &Locator, text: &str) -> Result<()> {
    click_center(page, locator).await?;
    page.keyboard().type_text(text, None).await?;
    Ok(())
}

// Even once a field is confirmed focused, its very first keystroke can still be dropped if
// Flutter's IME isn't quite ready the instant DOM focus lands. Verify what actually landed
// and, if it's wrong, clear it with real keyboard events and retype.
pub async fn type_into_verified(page: &Page, locator: &Locator, text: &str) -> Result<()> {
    for attempt in 1..=3 {
        if attempt > 1 {
            click_center(page, locator).await?;
            page.keyboard().press("Control+A", None).await?;
            page.keyboard().press("Backspace", None).await?;
        }
        type_into(page, locator, text).await?;
        // Confirmed live: checking input_value() only once, immediately after typing, can race
        // this field's own debounced autocomplete re-render -- the value hasn't propagated back
        // into the accessibility tree yet, making a genuinely successful type look like a drop
        // and triggering an unnecessary retry-click right into that re-render's mid-churn
        // window (where bounding_box_when_stable can then fail to see a stable, nonzero box for
        // the very same element). Give it a moment to settle before concluding it didn't land.
        let settle_deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        loop {
            if locator.input_value(None).await.unwrap_or_default() == text {
                return Ok(());
            }
            if tokio::time::Instant::now() >= settle_deadline {
                break;
            }
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
    }
    Err(anyhow!("Typed \"{text}\" but the field's value never matched after 3 attempts."))
}

pub async fn enable_flutter_accessibility(page: &Page) -> Result<()> {
    let toggle = page.get_by_role(
        AriaRole::Button,
        Some(GetByRoleOptions::default().name("Enable accessibility").exact(false)),
    );
    let visible = expect(toggle.clone())
        .with_timeout(Duration::from_secs(15))
        .to_be_visible()
        .await;
    if visible.is_ok() {
        toggle.dispatch_event("click", None).await?;
    }
    Ok(())
}

/// Playwright JS locators accept a RegExp for name/text matching; this crate's `name`/
/// `has_text` options only take a plain string. Where JS code used a regex (case-
/// insensitivity, alternation, partial matches), fetch every candidate and check its
/// accessible name (aria-label, falling back to inner text) against a real `Regex` in
/// application code instead -- the same "enumerate and check manually" approach already
/// used throughout this suite for elements with no usable accessible name at all.
pub async fn first_matching(candidates: &Locator, pattern: &Regex) -> Result<Option<Locator>> {
    // Confirmed live: a per-item timeout alone isn't enough. With enough candidates on a large
    // page (accumulated content from earlier steps in a long flow), even a *bounded* per-item
    // cost adds up to minutes with no cap on the total, which starves callers like
    // text_matching_within -- their own deadline check only runs *between* calls to this
    // function, never during one, so a slow-but-not-technically-hung scan can eat the whole
    // caller-side timeout budget without that caller ever getting a chance to notice. Cap the
    // entire scan (count + every item) as one unit; a scan that doesn't finish in time is
    // treated as "no match found this pass" rather than an error, so a polling caller just
    // retries on its next iteration instead of the whole call failing outright.
    let scan: Result<Option<Locator>> = tokio::time::timeout(Duration::from_secs(8), async {
        let count = candidates.count().await?;
        for i in 0..count as i32 {
            let item = candidates.nth(i);
            // A still-rendering/mutating page (e.g. right after a click that updates the DOM)
            // can leave `nth(i)` pointing at a since-detached node, which this crate's
            // connection layer sometimes hangs on indefinitely rather than erroring -- give
            // each item its own short timeout and skip it on failure/timeout rather than let
            // one stale node wedge the scan.
            let matched = tokio::time::timeout(Duration::from_millis(1000), async {
                let name = match item.get_attribute("aria-label").await.ok().flatten() {
                    Some(label) if !label.is_empty() => label,
                    _ => item.inner_text().await.unwrap_or_default(),
                };
                if pattern.is_match(&name) {
                    return true;
                }
                // Some controls (a segmented mode toggle, a value-display button) expose their
                // real name on a nested child -- e.g. `button > group("Offline")` -- rather
                // than on the element's own aria-label or text content, so the cheap check
                // above finds nothing. Fall back to the full nested accessibility tree,
                // checking the pattern against each individually quoted name in it (not the
                // whole multi-line blob, which an anchored ^...$ pattern would never match).
                if name.is_empty() {
                    if let Ok(snapshot) = item.aria_snapshot(None).await {
                        let quoted = Regex::new(r#""([^"]*)""#).unwrap();
                        if quoted.captures_iter(&snapshot).any(|c| pattern.is_match(&c[1])) {
                            return true;
                        }
                    }
                }
                false
            })
            .await;
            if matched == Ok(true) {
                return Ok(Some(item));
            }
        }
        Ok(None)
    })
    .await
    .unwrap_or(Ok(None));
    scan
}

pub async fn button_matching(page: &Page, pattern: &Regex) -> Result<Locator> {
    let buttons = page.get_by_role(AriaRole::Button, None);
    first_matching(&buttons, pattern)
        .await?
        .ok_or_else(|| anyhow!("Could not find a button matching {pattern}."))
}

/// `Locator::get_by_text` takes a plain substring, not a regex, so it can't express an
/// alternation like "incorrect|malformed|expired". Try each candidate substring in turn and
/// return whichever one actually matches something. A single immediate count() check isn't
/// enough here -- the text this looks for (an error toast, a validation message) often
/// hasn't rendered yet right after the action that triggers it, so this polls for up to
/// `timeout` rather than checking once and giving up.
pub async fn text_matching_any(page: &Page, candidates: &[&str], timeout: Duration) -> Result<Locator> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        for candidate in candidates {
            // Flutter mirrors on-screen text into a hidden `flt-announcement-polite` live
            // region for screen readers, so a plain get_by_text often resolves to two
            // elements (the announcement and the real, visible one) -- take the first to
            // avoid a strict-mode violation on to_be_visible().
            let locator = page.get_by_text(candidate, false).first();
            if locator.count().await? > 0 {
                return Ok(locator);
            }
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(anyhow!("Could not find text matching any of {candidates:?} within {timeout:?}."));
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// `Page::wait_for_url` takes a glob-style string, not a regex, and this suite's TS code
/// relies on regex URL matching throughout -- poll `page.url()` against a real `Regex`
/// instead, for the same matching semantics as the JS version.
pub async fn wait_for_url_matching(page: &Page, pattern: &Regex, timeout: Duration) -> Result<()> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if pattern.is_match(&page.url()) {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(anyhow!(
                "Timed out waiting for the URL to match {pattern}. Last URL: {}",
                page.url()
            ));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// The broadest possible candidate pool for a regex text search -- every Flutter semantics
/// node on the page, matching the `document.querySelectorAll('flt-semantics')` technique
/// used throughout this suite's live exploration. Used wherever TS code called
/// `page.getByText(regexPattern)` for something that isn't a button/textbox/etc, since
/// `Locator::get_by_text` here only accepts a plain substring.
pub fn all_text(page: &Page) -> Locator {
    page.locator("flt-semantics")
}

// A single first_matching scan can run before the target text has actually rendered (the
// page is still loading, a dialog is mid-transition, etc.) -- poll for up to `timeout`
// rather than failing on one early scan, mirroring Playwright's own auto-retrying `expect`.
pub async fn text_matching(page: &Page, pattern: &Regex) -> Result<Locator> {
    text_matching_within(page, pattern, Duration::from_secs(10)).await
}

pub async fn text_matching_within(page: &Page, pattern: &Regex, timeout: Duration) -> Result<Locator> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if let Some(found) = first_matching(&all_text(page), pattern).await? {
            return Ok(found);
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(anyhow!("Could not find text matching {pattern} within {timeout:?}."));
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

// This form's fields don't render in visual reading order in Flutter's semantics tree, and
// there's no aria-labelledby linking a label to its input, so locate each field by whichever
// textbox sits just below/beside its visible label text instead, matching how a sighted user
// would find it.
pub async fn textbox_near_label(page: &Page, label_pattern: &Regex) -> Result<Locator> {
    let label = text_matching(page, label_pattern).await?;
    textbox_near_locator(page, &label).await
}

// Same lookup as textbox_near_label, but starting from an already-resolved label locator --
// needed when the label text itself isn't unique on the page (e.g. two different sections
// both have an "Instructions" field) and the caller has already disambiguated which one via
// some other means (e.g. text_below anchored to a nearby unique landmark).
pub async fn textbox_near_locator(page: &Page, label: &Locator) -> Result<Locator> {
    let label_box = label.bounding_box().await?.ok_or_else(|| anyhow!("Could not read a bounding box for the label."))?;

    let textboxes = page.get_by_role(AriaRole::Textbox, None);
    let count = textboxes.count().await?;
    let mut best: Option<(i32, f64)> = None;
    for i in 0..count as i32 {
        let box_ = match tokio::time::timeout(Duration::from_millis(800), textboxes.nth(i).bounding_box()).await {
            Ok(Ok(Some(b))) => b,
            _ => continue,
        };
        let vertical_gap = box_.y - label_box.y;
        if !(0.0..=120.0).contains(&vertical_gap) {
            continue; // must sit just below the label
        }
        let horizontal_gap = (label_box.x - (box_.x + box_.width))
            .max(box_.x - (label_box.x + label_box.width))
            .max(0.0);
        let distance = vertical_gap + horizontal_gap;
        if best.map_or(true, |(_, d)| distance < d) {
            best = Some((i, distance));
        }
    }
    let (index, _) = best.ok_or_else(|| anyhow!("Could not find an input near the given label."))?;
    Ok(textboxes.nth(index))
}

// `getByText(pattern).first()` picks whichever DOM-order match comes first -- fine when a
// pattern is unique on the page, wrong once it isn't. Same "nearest below an anchor"
// approach as textbox_near_label, generalized to any text pattern.
pub async fn text_below(page: &Page, anchor: &Locator, pattern: &Regex, max_gap: f64) -> Result<Locator> {
    // Confirmed live: an anchor that was on-screen when the caller captured it can have
    // scrolled out of view by the time this runs (e.g. after filling in fields further up the
    // same form) -- bounding_box() then returns None since the element isn't currently
    // rendered. Scroll it back into view first rather than fail on an otherwise-valid anchor.
    let _ = tokio::time::timeout(Duration::from_secs(5), anchor.scroll_into_view_if_needed()).await;
    let anchor_box = anchor
        .bounding_box()
        .await?
        .ok_or_else(|| anyhow!("Could not read a bounding box for the anchor element."))?;

    let candidates = all_text(page);
    let count = candidates.count().await?;
    let mut best: Option<(i32, f64)> = None;
    for i in 0..count as i32 {
        let item = candidates.nth(i);
        let name = match item.get_attribute("aria-label").await.ok().flatten() {
            Some(label) if !label.is_empty() => label,
            _ => item.inner_text().await.unwrap_or_default(),
        };
        if !pattern.is_match(&name) {
            continue;
        }
        let Some(box_) = item.bounding_box().await? else { continue };
        let gap = box_.y - anchor_box.y;
        if !(0.0..=max_gap).contains(&gap) {
            continue;
        }
        if best.map_or(true, |(_, g)| gap < g) {
            best = Some((i, gap));
        }
    }
    let (index, _) = best.ok_or_else(|| anyhow!("Could not find text matching {pattern} below the anchor element."))?;
    Ok(candidates.nth(index))
}

// Several icon-only action buttons in this app have no accessible name at all, so they can't
// be targeted by role+name. They do sit reliably on the same row as some labelled anchor,
// just to its right.
pub async fn button_right_of(page: &Page, anchor_box: &BoundingBox) -> Result<Locator> {
    let buttons = page.get_by_role(AriaRole::Button, None);
    let count = buttons.count().await?;
    let mut best: Option<(i32, f64)> = None;
    for i in 0..count as i32 {
        let Some(box_) = buttons.nth(i).bounding_box().await? else { continue };
        let vertical_gap = (box_.y - anchor_box.y).abs();
        if vertical_gap > 40.0 {
            continue; // same row only
        }
        if box_.x < anchor_box.x + anchor_box.width {
            continue; // must sit to the right
        }
        let distance = vertical_gap + (box_.x - (anchor_box.x + anchor_box.width));
        if best.map_or(true, |(_, d)| distance < d) {
            best = Some((i, distance));
        }
    }
    let (index, _) = best.ok_or_else(|| anyhow!("Could not find a button to the right of the anchor element."))?;
    Ok(buttons.nth(index))
}

// This app's custom Flutter dropdowns ("Please select...") don't use native <select> or
// aria-labelledby linking a label to its control, so the closest "Please select..." text
// below a given label is the most reliable way to find one.
pub async fn dropdown_near_label(page: &Page, label_pattern: &Regex) -> Result<Locator> {
    let label = text_matching(page, label_pattern).await?;
    let label_box = label
        .bounding_box()
        .await?
        .ok_or_else(|| anyhow!("Could not find a label matching {label_pattern}."))?;

    let please_select = Regex::new(r"(?i)^Please select\.\.\.$").unwrap();
    let dropdowns = all_text(page);
    let count = dropdowns.count().await?;
    let mut best: Option<(i32, f64)> = None;
    for i in 0..count as i32 {
        let item = dropdowns.nth(i);
        let name = match item.get_attribute("aria-label").await.ok().flatten() {
            Some(label) if !label.is_empty() => label,
            _ => item.inner_text().await.unwrap_or_default(),
        };
        if !please_select.is_match(&name) {
            continue;
        }
        let Some(box_) = item.bounding_box().await? else { continue };
        let vertical_gap = box_.y - label_box.y;
        if !(0.0..=100.0).contains(&vertical_gap) {
            continue;
        }
        let horizontal_gap = (label_box.x - (box_.x + box_.width))
            .max(box_.x - (label_box.x + label_box.width))
            .max(0.0);
        if horizontal_gap > 300.0 {
            continue; // must sit in roughly the same column
        }
        let distance = vertical_gap + horizontal_gap;
        if best.map_or(true, |(_, d)| distance < d) {
            best = Some((i, distance));
        }
    }
    let (index, _) = best.ok_or_else(|| anyhow!("Could not find a dropdown near a label matching {label_pattern}."))?;
    Ok(dropdowns.nth(index))
}

// Opening one of this app's dropdowns replaces the underlying form's semantics tree with the
// overlay's while it's open, and the option buttons' visible text isn't real DOM text content
// (their accessible name lives only in aria-label) -- so options must be matched by role and
// name, never plain text. Opening the overlay can also silently fail to render on the first
// click with no visible difference from normal render latency, so this waits generously for
// the option rather than retrying the click itself.
pub async fn select_dropdown_option(page: &Page, dropdown: &Locator, option_pattern: &Regex) -> Result<()> {
    click_center(page, dropdown).await?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    loop {
        let buttons = page.get_by_role(AriaRole::Button, None);
        if let Some(option) = first_matching(&buttons, option_pattern).await? {
            if option.is_visible().await.unwrap_or(false) {
                click_center(page, &option).await?;
                return Ok(());
            }
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(anyhow!("Dropdown option matching {option_pattern} never appeared."));
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

// Same overlay mechanics as select_dropdown_option, for dropdowns where any one option is
// fine and the exact label isn't worth hardcoding -- picks whichever option renders closest
// below/right of the dropdown itself.
pub async fn select_first_dropdown_option(page: &Page, dropdown: &Locator) -> Result<()> {
    let dropdown_box = dropdown
        .bounding_box()
        .await?
        .ok_or_else(|| anyhow!("Could not read a bounding box for the dropdown element."))?;
    click_center(page, dropdown).await?;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    loop {
        let buttons = page.get_by_role(AriaRole::Button, None);
        let count = buttons.count().await?;
        let mut best: Option<(i32, f64)> = None;
        for i in 0..count as i32 {
            let Some(box_) = buttons.nth(i).bounding_box().await? else { continue };
            if box_.x < dropdown_box.x - 10.0 || box_.x > dropdown_box.x + dropdown_box.width + 10.0 {
                continue;
            }
            if box_.y < dropdown_box.y || box_.y > dropdown_box.y + 300.0 {
                continue;
            }
            if best.map_or(true, |(_, y)| box_.y < y) {
                best = Some((i, box_.y));
            }
        }
        if let Some((index, _)) = best {
            click_center(page, &buttons.nth(index)).await?;
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(anyhow!("Dropdown options never rendered."));
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}
