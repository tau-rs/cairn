import type { InputHTMLAttributes } from "react";

export function Input({
  className = "",
  ...rest
}: InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      className={`w-full rounded-md border border-border bg-bg px-2.5 py-1.5 text-sm text-text placeholder:text-faint focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-accent ${className}`}
      {...rest}
    />
  );
}
