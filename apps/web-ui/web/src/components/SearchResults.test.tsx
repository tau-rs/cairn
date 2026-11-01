import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { SearchResults } from "./SearchResults";

describe("SearchResults", () => {
  it("defaults the header to Results with a count", () => {
    render(
      <SearchResults
        results={["a.md", "b.md"]}
        onOpen={vi.fn()}
        onClose={vi.fn()}
      />,
    );
    expect(screen.getByText("Results (2)")).toBeInTheDocument();
  });
  it("uses a custom title when provided", () => {
    render(
      <SearchResults
        results={["a.md"]}
        title="Tagged · rust"
        onOpen={vi.fn()}
        onClose={vi.fn()}
      />,
    );
    expect(screen.getByText("Tagged · rust (1)")).toBeInTheDocument();
  });
  it("opens a result on click", () => {
    const onOpen = vi.fn();
    render(
      <SearchResults results={["a.md"]} onOpen={onOpen} onClose={vi.fn()} />,
    );
    fireEvent.click(screen.getByRole("button", { name: "a.md" }));
    expect(onOpen).toHaveBeenCalledWith("a.md");
  });
  it("renders nothing when results is null", () => {
    const { container } = render(
      <SearchResults results={null} onOpen={vi.fn()} onClose={vi.fn()} />,
    );
    expect(container).toBeEmptyDOMElement();
  });
  it("renders a highlighted snippet when provided", () => {
    render(
      <SearchResults
        results={["a.md"]}
        snippets={{
          "a.md": { snippet: "the quick fox", highlights: [[4, 9]] },
        }}
        onOpen={vi.fn()}
        onClose={vi.fn()}
      />,
    );
    expect(screen.getByText("quick")).toBeInTheDocument(); // the matched segment
    expect(screen.getByRole("button", { name: "a.md" })).toHaveTextContent(
      "the quick fox",
    );
  });
  it("renders a searching state when loading with no results yet", () => {
    render(
      <SearchResults
        results={null}
        loading
        onOpen={vi.fn()}
        onClose={vi.fn()}
      />,
    );
    expect(
      screen.getByRole("status", { name: /searching/i }),
    ).toBeInTheDocument();
  });
  it("renders path-only when no snippet is provided", () => {
    render(
      <SearchResults results={["a.md"]} onOpen={vi.fn()} onClose={vi.fn()} />,
    );
    expect(screen.getByRole("button", { name: "a.md" })).toHaveTextContent(
      "a.md",
    );
  });
});
