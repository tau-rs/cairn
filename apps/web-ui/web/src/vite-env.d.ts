/// <reference types="vite/client" />

// TypeScript 6 requires every bare side-effect import to resolve to a real type
// declaration. `@fontsource-variable/inter` ships only CSS (its `.` export maps
// to `index.css`, with no bundled `.d.ts`), so declare it as an ambient
// side-effect module. CSS path imports (`*.css`) are already covered by
// `vite/client` above.
declare module "@fontsource-variable/inter";
