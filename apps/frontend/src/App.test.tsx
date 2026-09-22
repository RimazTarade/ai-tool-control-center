import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";

beforeEach(() => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("review gate", () => {
  it("keeps discoveries pending until the user imports one", async () => {
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    render(<App />);
    expect(await screen.findByText(/all records are synthetic/i)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /review queue/i }));
    const imports = await screen.findAllByRole("button", { name: "Import" });
    await user.click(imports[0]);
    await user.click(screen.getByRole("button", { name: /^inventory/i }));
    expect(screen.getByText("Example local runtime", { selector: "strong" })).toBeInTheDocument();
  });

  it("allows navigation while a scan is active", async () => {
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "Run scan" }));
    await user.click(screen.getByRole("button", { name: "Run" }));
    expect(screen.getByLabelText(/scan progress/i)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Run scan" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: /^settings/i }));
    expect(screen.getByRole("heading", { level: 1, name: "Settings" })).toBeInTheDocument();
    expect(screen.getByLabelText(/scan progress/i)).toBeInTheDocument();
  });
});

describe("browser demo health integration", () => {
  it("health_ui_does_not_treat_reviewed_as_healthy", async () => {
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    render(<App />);

    expect(await screen.findByText(/all records are synthetic/i)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /review queue/i }));
    const imports = await screen.findAllByRole("button", { name: "Import" });
    await user.click(imports[0]);

    await user.click(screen.getByRole("button", { name: /^health/i }));

    expect(await screen.findByText("Example local runtime", { selector: "strong" })).toBeInTheDocument();
    expect(screen.getByText("Not checked", { selector: "b" })).toBeInTheDocument();
    expect(screen.queryByText("Healthy")).not.toBeInTheDocument();
  });
});
describe("browser demo scan simulation", () => {
  it("does not touch the filesystem and runs progress, a recoverable warning, pause/resume, and terminal", async () => {
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    render(<App />);

    await user.click(await screen.findByRole("button", { name: "Run scan" }));
    await user.click(screen.getByRole("button", { name: "Run" }));

    expect(screen.getByLabelText(/scan progress/i)).toBeInTheDocument();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(220 * 3);
    });
    expect(await screen.findByRole("alert")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Pause" }));
    expect(screen.getByLabelText(/scan progress/i)).toHaveTextContent("Paused");

    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByLabelText(/scan progress/i)).not.toBeInTheDocument());
    expect(await screen.findByText(/scan cancelled/i)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Run scan" }));
    expect(screen.getByRole("dialog", { name: "Run scan" })).toBeInTheDocument();
  });
});

describe("browser demo dependency integration", () => {
  it("dependency_ui_browser_demo_enforces_scanner_read_only", async () => {
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    render(<App />);

    expect(await screen.findByText(/all records are synthetic/i)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /review queue/i }));

    let imports = screen.queryAllByRole("button", { name: "Import" });
    while (imports.length > 0) {
      await user.click(imports[0]);
      imports = screen.queryAllByRole("button", { name: "Import" });
    }

    await user.click(screen.getByRole("button", { name: /^dependencies/i }));

    const scannerRow = await screen.findByTestId("dependency-row-demo-scanner");
    expect(scannerRow).toHaveTextContent(/scanner/i);
    expect(scannerRow).toHaveTextContent(/read.?only/i);
    expect(
      scannerRow.querySelector('button[aria-label="Delete"]'),
    ).toBeNull();
  });
});
