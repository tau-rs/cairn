import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { TagsPanel } from "./TagsPanel";

const tags = [
  { tag: "rust", count: 2 },
  { tag: "ideas", count: 1 },
];

describe("TagsPanel", () => {
  it("renders each tag with its count", () => {
    render(<TagsPanel tags={tags} activeTag={null} onSelect={vi.fn()} />);
    const rust = screen.getByRole("button", { name: "filter by tag rust" });
    expect(rust).toHaveTextContent("rust");
    expect(rust).toHaveTextContent("2");
  });
  it("calls onSelect with the tag when clicked", () => {
    const onSelect = vi.fn();
    render(<TagsPanel tags={tags} activeTag={null} onSelect={onSelect} />);
    fireEvent.click(
      screen.getByRole("button", { name: "filter by tag ideas" }),
    );
    expect(onSelect).toHaveBeenCalledWith("ideas");
  });
  it("marks the active tag pressed", () => {
    render(<TagsPanel tags={tags} activeTag="rust" onSelect={vi.fn()} />);
    expect(
      screen.getByRole("button", { name: "filter by tag rust" }),
    ).toHaveAttribute("aria-pressed", "true");
  });
  it("collapses the list via the header toggle", () => {
    render(<TagsPanel tags={tags} activeTag={null} onSelect={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "toggle tags" }));
    expect(
      screen.queryByRole("button", { name: "filter by tag rust" }),
    ).not.toBeInTheDocument();
  });
  it("renders nothing when there are no tags", () => {
    const { container } = render(
      <TagsPanel tags={[]} activeTag={null} onSelect={vi.fn()} />,
    );
    expect(container).toBeEmptyDOMElement();
  });
});
