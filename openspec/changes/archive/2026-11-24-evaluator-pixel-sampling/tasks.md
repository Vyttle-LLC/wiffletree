## 1. Shared module

- [x] 1.1 `pixel_sampling`: the tool's schema, PNG reading by the screenshot's containment rules plus no links, hex, luminance and contrast.
- [x] 1.2 Both drivers list `sample_pixels` and route it to the module; the allow lists and start checks follow from the lists.

## 2. Docs and tests

- [x] 2.1 `docs/FEATURES.md` and the coordinator-facing `start_evaluation` text name the tool.
- [x] 2.2 Tests: known colours, WCAG reference contrast, each refusal, the closed schema, both drivers listing it.
