import { describe, it, expect, vi } from "vitest";
import { useState } from "react";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SearchBar } from "./SearchBar";

describe("SearchBar", () => {
  it("submits the live query on Enter", async () => {
    const onSearch = vi.fn();
    function Harness() {
      const [v, setV] = useState("");
      return <SearchBar value={v} onChange={setV} onSearch={onSearch} />;
    }
    render(<Harness />);
    await userEvent.type(
      screen.getByPlaceholderText(/search/i),
      "target{enter}",
    );
    expect(onSearch).toHaveBeenCalledWith("target");
  });
});
