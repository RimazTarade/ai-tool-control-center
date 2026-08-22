import { expect, test, type Page } from "@playwright/test";

// The browser demo mode (Task 8) drives its own synthetic scan lifecycle on a
// 220ms interval: progress ticks, one deliberately recoverable scanner
// failure at step 3, then completion at step 6. These tests exercise that
// real timer-driven state machine in an actual browser — no mocking.

async function openAndStartQuickScan(page: Page) {
  await page.getByRole("button", { name: "Run scan" }).click();
  await expect(page.getByRole("dialog", { name: "Run scan" })).toBeVisible();
  await page.getByRole("button", { name: "Run", exact: true }).click();
}

function scanbar(page: Page) {
  return page.getByRole("region", { name: "Scan progress" });
}

async function readCompletedUnits(page: Page): Promise<number> {
  const text = (await scanbar(page).locator("span").innerText()).trim();
  const match = text.match(/(\d+)\s*\/\s*\d+/);
  if (!match) throw new Error(`could not parse completed units from "${text}"`);
  return Number(match[1]);
}

test("navigation and review remain interactive during a slow scan", async ({ page }) => {
  await page.goto("/");
  await openAndStartQuickScan(page);

  await expect(scanbar(page).getByText(/Quick scan/i)).toBeVisible();
  await expect(scanbar(page).getByText(/Running/i)).toBeVisible();

  // Navigate to another page while the scan is actively running.
  await page.getByRole("button", { name: "Review Queue" }).click();
  await expect(page.getByRole("main")).toBeVisible();
  await expect(page.getByRole("heading", { name: /discoveries need a decision/i })).toBeVisible();

  // The scan must still be running after that navigation.
  await expect(scanbar(page).getByText(/Running/i)).toBeVisible();

  // Interact with the review queue itself (a real state-changing action)
  // while the scan is still in flight.
  await page.getByRole("button", { name: "Keep unknown" }).first().click();

  // Still running after the review interaction too.
  await expect(scanbar(page).getByText(/Running/i)).toBeVisible();

  // Wait for the demo's deliberate scanner failure to prove the UI kept
  // ticking underneath the interaction, without terminating the scan.
  await expect(scanbar(page).getByText(/demo\.scanner/i).first()).toBeVisible();
  await expect(scanbar(page).getByText(/Running/i)).toBeVisible();
});

test("pause halts progress, resume continues it, cancel reaches a terminal state, and a second scan can start", async ({ page }) => {
  await page.goto("/");
  await openAndStartQuickScan(page);

  await expect(scanbar(page).getByText(/Running/i)).toBeVisible();

  // Wait for completed units to advance at least once.
  await expect
    .poll(async () => readCompletedUnits(page), { timeout: 10_000 })
    .toBeGreaterThan(0);

  await page.getByRole("button", { name: "Pause" }).click();
  await expect(scanbar(page).getByText(/Paused/i)).toBeVisible();

  const pausedUnits = await readCompletedUnits(page);

  // Wait well past one demo tick interval (220ms) to prove no further
  // progress lands while paused.
  await page.waitForTimeout(700);
  expect(await readCompletedUnits(page)).toBe(pausedUnits);

  await page.getByRole("button", { name: "Resume" }).click();
  await expect(scanbar(page).getByText(/Running/i)).toBeVisible();

  await expect
    .poll(async () => readCompletedUnits(page), { timeout: 10_000 })
    .toBeGreaterThan(pausedUnits);

  await page.getByRole("button", { name: "Cancel" }).click();

  await expect(page.getByRole("status").filter({ hasText: /Scan cancelled/i })).toBeVisible();
  await expect(scanbar(page)).toHaveCount(0);

  // Restart: a second scan can be started successfully after the terminal state.
  await openAndStartQuickScan(page);
  await expect(scanbar(page).getByText(/Running/i)).toBeVisible();
});

test("a recoverable scanner failure surfaces a warning without terminating the scan", async ({ page }) => {
  await page.goto("/");
  await openAndStartQuickScan(page);

  await expect(scanbar(page).getByText(/Running/i)).toBeVisible();

  // The demo deliberately fails one scanner partway through. Assert the
  // warning appears and the scan stays Running — one scanner failure must
  // not terminate the whole scan.
  await expect(page.getByRole("alert").filter({ hasText: /demo\.scanner.*demo_recoverable/i })).toBeVisible({ timeout: 10_000 });
  await expect(scanbar(page).getByText(/Running/i)).toBeVisible();

  const unitsAtWarning = await readCompletedUnits(page);

  // Prove the scan keeps making progress after the recoverable warning.
  await expect
    .poll(async () => readCompletedUnits(page), { timeout: 10_000 })
    .toBeGreaterThan(unitsAtWarning);
  await expect(scanbar(page).getByText(/Running/i)).toBeVisible();
});
