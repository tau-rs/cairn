import { describe, it, expect, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { DaemonSettings } from "./DaemonSettings";
import { loadDaemonConfig, saveDaemonConfig } from "../client/daemonConfig";

describe("DaemonSettings", () => {
  beforeEach(() => localStorage.clear());

  it("prefills the fields from the stored config", () => {
    saveDaemonConfig({ url: "http://localhost:7777", token: "tok" });
    render(<DaemonSettings />);
    expect(screen.getByLabelText(/daemon url/i)).toHaveValue(
      "http://localhost:7777",
    );
    expect(screen.getByLabelText(/token/i)).toHaveValue("tok");
  });

  it("persists edits to the daemon config on save", () => {
    render(<DaemonSettings />);
    fireEvent.change(screen.getByLabelText(/daemon url/i), {
      target: { value: "http://localhost:9000" },
    });
    fireEvent.change(screen.getByLabelText(/token/i), {
      target: { value: "abc" },
    });
    fireEvent.click(screen.getByRole("button", { name: /save/i }));
    expect(loadDaemonConfig()).toEqual({
      url: "http://localhost:9000",
      token: "abc",
    });
  });

  it("tells the user the change takes effect after a reload", () => {
    render(<DaemonSettings />);
    expect(screen.getByText(/reload/i)).toBeInTheDocument();
  });
});
