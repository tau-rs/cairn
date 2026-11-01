import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ErrorBoundary } from "./ErrorBoundary";

function Boom({ when = true }: { when?: boolean }) {
  if (when) throw new Error("kaboom");
  return <div>safe child</div>;
}

describe("ErrorBoundary", () => {
  // React logs caught errors to console.error; silence it for clean output.
  let spy: ReturnType<typeof vi.spyOn>;
  beforeEach(() => {
    spy = vi.spyOn(console, "error").mockImplementation(() => {});
  });
  afterEach(() => spy.mockRestore());

  it("renders children when nothing throws", () => {
    render(
      <ErrorBoundary>
        <div>safe child</div>
      </ErrorBoundary>,
    );
    expect(screen.getByText("safe child")).toBeInTheDocument();
  });

  it("renders the fallback instead of unmounting when a child throws", () => {
    render(
      <ErrorBoundary>
        <Boom />
      </ErrorBoundary>,
    );
    // The thrown child is gone; the recoverable fallback is shown.
    expect(screen.queryByText("safe child")).not.toBeInTheDocument();
    expect(screen.getByText(/something went wrong/i)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /reload/i })).toBeInTheDocument();
  });

  it("reports the error and component stack via onError", () => {
    const onError = vi.fn();
    render(
      <ErrorBoundary onError={onError}>
        <Boom />
      </ErrorBoundary>,
    );
    expect(onError).toHaveBeenCalledTimes(1);
    const [err, info] = onError.mock.calls[0];
    expect(err).toBeInstanceOf(Error);
    expect((err as Error).message).toBe("kaboom");
    // componentStack is the diagnostic React provides.
    expect(info).toHaveProperty("componentStack");
    expect(typeof info.componentStack).toBe("string");
  });

  it("uses a custom fallback render-prop and can reset on retry", async () => {
    let shouldThrow = true;
    function Toggle() {
      if (shouldThrow) throw new Error("kaboom");
      return <div>recovered child</div>;
    }
    render(
      <ErrorBoundary
        fallback={(reset) => <button onClick={reset}>retry-widget</button>}
      >
        <Toggle />
      </ErrorBoundary>,
    );
    expect(screen.getByText("retry-widget")).toBeInTheDocument();
    // Fix the underlying condition, then retry resets the boundary.
    shouldThrow = false;
    await userEvent.click(screen.getByText("retry-widget"));
    expect(screen.getByText("recovered child")).toBeInTheDocument();
  });
});
