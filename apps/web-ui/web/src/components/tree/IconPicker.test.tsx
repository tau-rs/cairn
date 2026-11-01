import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { IconPicker } from "./IconPicker";

function open(kind: "folder" | "note", onChange = vi.fn()) {
  render(
    <IconPicker
      targetKind={kind}
      value={{}}
      onChange={onChange}
      trigger={<button>set icon</button>}
    />,
  );
  return onChange;
}

describe("IconPicker", () => {
  it("opens on trigger click and shows both tabs", async () => {
    open("note");
    await userEvent.click(screen.getByText("set icon"));
    expect(screen.getByRole("tab", { name: "Emoji" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Icons" })).toBeInTheDocument();
  });

  it("selecting an emoji calls onChange with an emoji IconRef", async () => {
    const onChange = open("note");
    await userEvent.click(screen.getByText("set icon"));
    await userEvent.click(screen.getByRole("button", { name: "books 📚" }));
    expect(onChange).toHaveBeenCalledWith({
      icon: { kind: "emoji", value: "📚" },
    });
  });

  it("filters emoji by search", async () => {
    open("note");
    await userEvent.click(screen.getByText("set icon"));
    await userEvent.type(screen.getByPlaceholderText("Search emoji…"), "idea");
    expect(screen.getByRole("button", { name: "bulb 💡" })).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "books 📚" }),
    ).not.toBeInTheDocument();
  });

  it("selecting a lucide icon uses the selected color", async () => {
    const onChange = open("note");
    await userEvent.click(screen.getByText("set icon"));
    await userEvent.click(screen.getByRole("tab", { name: "Icons" }));
    await userEvent.click(
      screen.getByRole("button", { name: "color #e5484d" }),
    );
    await userEvent.click(screen.getByRole("button", { name: "icon star" }));
    expect(onChange).toHaveBeenCalledWith({
      icon: { kind: "lucide", name: "star", color: "#e5484d" },
    });
  });

  it("Remove clears the icon", async () => {
    const onChange = open("note");
    await userEvent.click(screen.getByText("set icon"));
    await userEvent.click(screen.getByRole("button", { name: "Remove" }));
    expect(onChange).toHaveBeenCalledWith({});
  });

  it("shows the Folder color footer only for folders", async () => {
    open("folder");
    await userEvent.click(screen.getByText("set icon"));
    expect(screen.getByText("Folder color")).toBeInTheDocument();
  });

  it("hides the Folder color footer for notes", async () => {
    open("note");
    await userEvent.click(screen.getByText("set icon"));
    expect(screen.queryByText("Folder color")).not.toBeInTheDocument();
  });
});
