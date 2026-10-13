import { expect, test, type Page } from "@playwright/test";

const runtimeName = "Example local runtime";
const mcpName = "Example MCP registration";

async function navigate(page: Page, name: string) {
  await page.getByRole("button", { name, exact: true }).click();
  await expect(page.getByRole("main")).toBeVisible();
}

async function importDemoInstallations(page: Page, count: 1 | 2 = 2) {
  await page.goto("/");
  await expect(page.getByRole("main")).toBeVisible();

  await navigate(page, "Review Queue");

  for (let index = 0; index < count; index += 1) {
    const importButton = page.getByRole("button", { name: "Import" }).first();
    await expect(importButton).toBeVisible();
    await importButton.click();
  }
}

function articleContaining(page: Page, text: string) {
  return page.getByRole("article").filter({ hasText: text }).first();
}

function relationshipRows(page: Page) {
  return page
    .getByRole("region", { name: "Dependency relationships" })
    .locator("[data-dependency-row]");
}

function dependencyEdges(page: Page) {
  return page
    .getByRole("img", { name: "Dependency graph" })
    .locator("[data-dependency-edge]");
}

test("never checked stays unknown until explicit action", async ({ page }) => {
  await importDemoInstallations(page, 1);
  await navigate(page, "Health");

  const healthRow = articleContaining(page, runtimeName);

  await expect(healthRow).toContainText("Not checked");
  await expect(
    page.getByText("Synthetic demo result - not observed on this computer"),
  ).toHaveCount(0);

  // A single demo run currently reaches its terminal event by 800 ms. Waiting
  // beyond that proves merely visiting Health does not schedule one.
  await page.waitForTimeout(1_000);

  await expect(healthRow).toContainText("Not checked");
  await expect(
    page.getByText("Synthetic demo result - not observed on this computer"),
  ).toHaveCount(0);

  await navigate(page, "Inventory");
  await expect(articleContaining(page, runtimeName)).toContainText("Not checked");

  await navigate(page, "Overview");

  const unknownMetric = page
    .getByRole("article")
    .filter({ hasText: "Unknown" })
    .first();

  await expect(unknownMetric).toContainText("01");

  await navigate(page, "Health");
  await expect(articleContaining(page, runtimeName)).toContainText("Not checked");
});

test("single installation health run projects authoritative detail into Health Inventory and Overview", async ({
  page,
}) => {
  await importDemoInstallations(page, 1);
  await navigate(page, "Health");

  const healthRow = articleContaining(page, runtimeName);

  await healthRow.getByRole("button", { name: "Run checks" }).click();

  // Explicit execution must expose an observable transient state rather than
  // jumping directly from unknown to terminal.
  await expect(
    healthRow.getByText("Checking", { exact: true }),
  ).toBeVisible();

  await expect(healthRow).toContainText("Healthy");

  await healthRow
    .getByRole("button", { name: new RegExp(runtimeName, "i") })
    .click();

  const detail = page.getByRole("region", {
    name: "Installation health detail",
  });

  await expect(detail).toBeVisible();
  await expect(detail).toContainText("path_exists");
  await expect(detail).toContainText("Required");
  await expect(detail).toContainText("State: Healthy");
  await expect(detail).toContainText("Duration: 10 ms");
  await expect(detail).toContainText("Observed:");
  await expect(detail).toContainText(
    "Synthetic demo result - not observed on this computer",
  );

  await navigate(page, "Inventory");

  const inventoryRow = articleContaining(page, runtimeName);
  await expect(inventoryRow).toContainText("Healthy");

  await navigate(page, "Overview");

  const healthyMetric = page
    .getByRole("article")
    .filter({ hasText: "Healthy" })
    .first();

  await expect(healthyMetric).toContainText("01");

  const unknownMetric = page
    .getByRole("article")
    .filter({ hasText: "Unknown" })
    .first();

  await expect(unknownMetric).toContainText("00");
});

test("Check All cancellation preserves settled work and prevents later synthetic results", async ({
  page,
}) => {
  await importDemoInstallations(page, 2);
  await navigate(page, "Health");

  const runtimeRow = articleContaining(page, runtimeName);
  const mcpRow = articleContaining(page, mcpName);

  await expect(runtimeRow).toContainText("Not checked");
  await expect(mcpRow).toContainText("Not checked");

  await page.getByRole("button", { name: "Check All" }).click();

  const progress = page.getByRole("region", {
    name: "Health check progress",
  });

  await expect(progress).toBeVisible();
  await expect(runtimeRow).toContainText("Checking");
  await expect(mcpRow).toContainText("Checking");

  // Wait on visible state, not a guessed timer: one installation must settle
  // while the aggregate run is still cancellable.
  await expect(runtimeRow).toContainText("Healthy");
  await expect(progress.getByRole("button", { name: "Cancel" })).toBeVisible();

  await progress.getByRole("button", { name: "Cancel" }).click();

  await expect(page.getByText("Cancelled", { exact: true })).toBeVisible();

  // Work that settled before cancellation remains authoritative.
  await expect(runtimeRow).toContainText("Healthy");

  // Work that had not settled must stay unknown.
  await expect(mcpRow).toContainText("Not checked");

  await runtimeRow
    .getByRole("button", { name: new RegExp(runtimeName, "i") })
    .click();

  const detail = page.getByRole("region", {
    name: "Installation health detail",
  });

  await expect(detail).toContainText(
    "Synthetic demo result - not observed on this computer",
  );

  // Wait beyond all current synthetic timers. Cancellation must have cleared
  // future work instead of allowing another result to land afterward.
  await page.waitForTimeout(700);

  await expect(mcpRow).toContainText("Not checked");
});

test("dependency list is authoritative and the user editor adds and removes one persisted edge", async ({
  page,
}) => {
  await importDemoInstallations(page, 2);
  await navigate(page, "Dependencies");

  const relationshipRegion = page.getByRole("region", {
    name: "Dependency relationships",
  });

  await expect(
    relationshipRegion.getByRole("heading", { name: "Relationships" }),
  ).toBeVisible();

  await expect(relationshipRows(page)).toHaveCount(1);
  await expect(dependencyEdges(page)).toHaveCount(1);

  await page
    .getByRole("combobox", { name: "Source capability" })
    .selectOption({ label: `Source: ${runtimeName}` });

  await page
    .getByRole("combobox", { name: "Relationship type" })
    .selectOption({ label: "Type: Depends on" });

  await page
    .getByRole("combobox", { name: "Target capability" })
    .selectOption({ label: `Target: ${mcpName}` });

  await page.getByRole("button", { name: "Add relationship" }).click();

  const userRow = relationshipRows(page)
    .filter({ hasText: runtimeName })
    .filter({ hasText: "Depends on" })
    .filter({ hasText: mcpName })
    .filter({ hasText: "User" });

  await expect(userRow).toHaveCount(1);
  await expect(userRow).toContainText(runtimeName);
  await expect(userRow).toContainText("Depends on");
  await expect(userRow).toContainText(mcpName);
  await expect(userRow).toContainText("User");

  await expect(relationshipRows(page)).toHaveCount(2);
  await expect(dependencyEdges(page)).toHaveCount(2);

  const userEdge = page
    .getByRole("img", { name: "Dependency graph" })
    .locator(
      '[data-source-capability-id="cap-demo-1"][data-target-capability-id="cap-demo-2"][data-dependency-kind="depends_on"]',
    );

  await expect(userEdge).toHaveCount(1);

  await userRow.getByRole("button", { name: "Delete" }).click();

  await expect(userRow).toHaveCount(0);
  await expect(userEdge).toHaveCount(0);
  await expect(relationshipRows(page)).toHaveCount(1);
  await expect(dependencyEdges(page)).toHaveCount(1);
});

test("scanner dependency remains visibly read-only across navigation", async ({
  page,
}) => {
  await importDemoInstallations(page, 2);
  await navigate(page, "Dependencies");

  const scannerRow = relationshipRows(page)
    .filter({ hasText: "Scanner" })
    .first();

  await expect(scannerRow).toBeVisible();
  await expect(scannerRow).toContainText("Scanner");
  await expect(scannerRow).toContainText("Read-only");
  await expect(
    scannerRow.getByRole("button", { name: "Delete" }),
  ).toHaveCount(0);

  const relationshipCountBefore = await relationshipRows(page).count();
  const edgeCountBefore = await dependencyEdges(page).count();

  expect(edgeCountBefore).toBe(relationshipCountBefore);

  await navigate(page, "Overview");
  await navigate(page, "Dependencies");

  const scannerRowAfter = relationshipRows(page)
    .filter({ hasText: "Scanner" })
    .first();

  await expect(scannerRowAfter).toBeVisible();
  await expect(
    scannerRowAfter.getByRole("button", { name: "Delete" }),
  ).toHaveCount(0);

  await expect(relationshipRows(page)).toHaveCount(relationshipCountBefore);
  await expect(dependencyEdges(page)).toHaveCount(edgeCountBefore);
});

test("dependency graph contains only persisted relationships and invents no heuristic edges", async ({
  page,
}) => {
  await importDemoInstallations(page, 2);
  await navigate(page, "Dependencies");

  // The two reviewed demo records deliberately share the generic "Example"
  // naming pattern. Similar names and installation text must not produce any
  // graph geometry beyond the explicitly persisted scanner relationship.
  await expect(page.getByText(runtimeName, { exact: true }).first()).toBeVisible();
  await expect(page.getByText(mcpName, { exact: true }).first()).toBeVisible();

  const rows = relationshipRows(page);
  const edges = dependencyEdges(page);

  await expect(rows).toHaveCount(1);
  await expect(edges).toHaveCount(1);

  expect(await edges.count()).toBe(await rows.count());

  const graph = page.getByRole("img", { name: "Dependency graph" });

  await expect(
    graph.locator(
      '[data-source-capability-id="cap-demo-1"][data-target-capability-id="cap-demo-1"]',
    ),
  ).toHaveCount(0);

  await expect(
    graph.locator(
      '[data-source-capability-id="cap-demo-2"][data-target-capability-id="cap-demo-2"]',
    ),
  ).toHaveCount(0);

  await expect(
    graph.locator(
      '[data-source-capability-id="cap-demo-2"][data-target-capability-id="cap-demo-1"]',
    ),
  ).toHaveCount(0);
});
