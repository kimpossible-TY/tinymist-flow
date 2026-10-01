# Tasks

## 1. Native document variants

- [x] 1.1 Add opt-in CLI inputs and shared-focus sibling builders; verify CLI argument and focus-store tests.
- [x] 1.2 Compile both variants and route them on the same port; verify route/origin tests and concurrent live streams.

## 2. Viewer theme controller

- [x] 2.1 Add device-change listener and accessible System / Light / Dark overrides; verify controller tests and TypeScript checks.
- [x] 2.2 Preserve reading state and defer transitions during selection; verify disposal, selection, and transition tests.

## 3. Integration and delivery

- [x] 3.1 Enable managed previews and document the input contract; verify launcher tests and generated-doc consistency.
- [x] 3.2 Build frontend and release engine; verify targeted Rust tests, formatting, and runtime edits/focus for both palettes.
- [ ] 3.3 Install the validated bundled app and verify existing preview URLs; record any remaining OS consent or physical-device checks explicitly.
