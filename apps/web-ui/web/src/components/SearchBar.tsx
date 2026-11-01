import { Input } from "./ui/Input";

export function SearchBar(props: {
  value: string;
  onChange: (value: string) => void;
  onSearch: (query: string) => void;
}) {
  return (
    <div className="w-64">
      <Input
        placeholder="Search…"
        value={props.value}
        onChange={(e) => props.onChange(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") props.onSearch(props.value);
        }}
      />
    </div>
  );
}
