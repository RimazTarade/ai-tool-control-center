import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";
import App from "./App";

afterEach(cleanup);

describe("review gate", () => {
  it("keeps discoveries pending until the user imports one", async () => {
    const user = userEvent.setup();
    render(<App />);
    expect(await screen.findByText(/all records are synthetic/i)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /review queue/i }));
    const imports = await screen.findAllByRole("button", { name: "Import" });
    await user.click(imports[0]);
    await user.click(screen.getByRole("button", { name: /^inventory/i }));
    expect(screen.getByText("Example local runtime")).toBeInTheDocument();
  });

  it("allows navigation while a scan is active", async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.click(screen.getByRole("button", { name: /run quick scan/i }));
    expect(screen.getByLabelText(/scan progress/i)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /run quick scan/i })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: /^settings/i }));
    expect(screen.getByRole("heading", { level: 1, name: "Settings" })).toBeInTheDocument();
    expect(screen.getByLabelText(/scan progress/i)).toBeInTheDocument();
  });
});
